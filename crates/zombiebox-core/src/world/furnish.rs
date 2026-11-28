//! Deterministic room furnishing for authored Phase 3 blueprints.
//!
//! Furnishing runs after doors and stairs are authored. It reserves circulation
//! first, then places required fixtures in dependency/priority order and only
//! afterwards fills optional clutter. Every placement is checked against the
//! room mask, the reserved circulation spine, prop span rotation, window
//! sightlines and real interaction approaches. Wall-backed and relative
//! fixtures also check their physical bounds: a bed or bathtub is slid along
//! its backing normal until it sits a design gap from the wall, and the
//! translated collider must still clear walls, other fixtures and reserved
//! approaches. The final pass reruns reachability over the furnished walkable
//! mask, so optional clutter cannot close a room or trap a fixture.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use crate::Floor;
use crate::action::{CellAttachment, PropCover, PropFacing, PropId, PropSpan, Vec3, mix64};
use crate::spatial::WALL_HALF_THICKNESS;

use super::RoomKind;
use super::blueprint::{
    Align, BlueprintAnchor, BlueprintGrid, BlueprintKind, ChildFacing, FitError, LocalCell,
    LocalPlacementKey, LocalPropOffset, LootProfileId, ParentSide, PlacementStrategy, PlannedProp,
    RoomMeta, RuleKey, WallPreference,
};

const FURNISH_SALT: u64 = 0x4655_524e_4953_4831; // "FURNISH1"

const NEIGHBORS: [(i32, i32); 4] = [(0, 1), (-1, 0), (1, 0), (0, -1)];

/// Overlap tolerance in meters. Touching faces do not count as overlap.
const EPS: f32 = 0.005;

/// Open floor kept between unrelated furniture in living rooms and for
/// floor-open placements (coffee tables, dining tables, desks).
const CLUTTER_CLEARANCE_M: f32 = 0.5;

/// Candidate evaluations allowed for the bathroom fixture search.
const GROUP_SEARCH_BUDGET: u32 = 256;

/// Bathroom fixtures are placed as one bounded group. The order here is the
/// order the search visits them: tub first, then toilet, then sink.
const BATHROOM_GROUP: &[RuleKey] = &[RuleKey(40), RuleKey(41), RuleKey(42)];

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
    /// Optional clutter is placed after fixtures and seating groups, so it is
    /// the first thing to give way when a room runs out of floor.
    pub clutter: bool,
    pub loot: Option<LootProfileId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FurnishReport {
    pub placed: u16,
    pub optional_removed: u16,
}

/// A placed fixture in blueprint space. `offset` is the physical translation
/// from the tile-span pivot, in meters along `(u, v)`.
#[derive(Clone, Copy, Debug)]
struct PlacedProp {
    prop: PropId,
    anchor: BlueprintAnchor,
    facing: PropFacing,
    required: bool,
    offset: (f32, f32),
}

/// A candidate placement before it is committed.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    prop: PropId,
    anchor: BlueprintAnchor,
    facing: PropFacing,
    offset: (f32, f32),
}

impl Candidate {
    fn placed(self, required: bool) -> PlacedProp {
        PlacedProp {
            prop: self.prop,
            anchor: self.anchor,
            facing: self.facing,
            required,
            offset: self.offset,
        }
    }
}

/// Axis-aligned physical rectangle in blueprint meters.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    u0: f32,
    u1: f32,
    v0: f32,
    v1: f32,
}

impl Rect {
    fn center(self) -> (f32, f32) {
        ((self.u0 + self.u1) * 0.5, (self.v0 + self.v1) * 0.5)
    }

    fn overlaps(self, other: Rect) -> bool {
        self.u0 < other.u1 - EPS
            && other.u0 < self.u1 - EPS
            && self.v0 < other.v1 - EPS
            && other.v0 < self.v1 - EPS
    }
}

fn tile_rect(u: i32, v: i32) -> Rect {
    Rect {
        u0: u as f32 - 0.5,
        u1: u as f32 + 0.5,
        v0: v as f32 - 0.5,
        v1: v as f32 + 0.5,
    }
}

#[derive(Clone, Copy, Debug)]
struct PhysicalEntry {
    rect: Rect,
    floor: Floor,
}

#[derive(Clone, Default)]
struct Occupancy {
    floor: BTreeSet<LocalCell>,
    attachments: HashSet<(LocalCell, CellAttachment)>,
    physical: Vec<PhysicalEntry>,
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

    fn insert_physical(&mut self, rect: Rect, floor: Floor) {
        self.physical.push(PhysicalEntry { rect, floor });
    }

    fn physical_overlap(&self, rect: Rect, floor: Floor) -> bool {
        self.physical
            .iter()
            .any(|entry| entry.floor == floor && rect.overlaps(entry.rect))
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
        let group = group_rules_for(room.kind);
        if !group.is_empty() {
            let group_rules: Vec<&PropPlacementRule> = ordered
                .iter()
                .map(|(rule, _)| *rule)
                .filter(|rule| group.contains(&rule.key))
                .collect();
            let fixtures = place_group(
                seed,
                blueprint,
                &room,
                &room_cells,
                &mut reserved,
                &mut occupancy,
                &placed_by_rule,
                &group_rules,
            )?;
            for (rule, placed) in group_rules.iter().zip(fixtures) {
                record_placement(
                    &room,
                    rule,
                    0,
                    placed,
                    &mut placed_by_rule,
                    &mut placed_props,
                    &mut required_keys,
                );
            }
        }
        for (rule, _depth) in ordered {
            if group.contains(&rule.key) {
                continue;
            }
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
                    record_placement(
                        &room,
                        rule,
                        copy,
                        placed,
                        &mut placed_by_rule,
                        &mut placed_props,
                        &mut required_keys,
                    );
                }
            }
        }
    }

    blueprint.props.extend(placed_props);
    let removed = prune_unreachable(blueprint, &required_keys)?;
    anchor_signs(blueprint)?;
    Ok(FurnishReport {
        placed: blueprint.props.len() as u16,
        optional_removed: removed,
    })
}

fn record_placement(
    room: &RoomMeta,
    rule: &PropPlacementRule,
    copy: u8,
    placed: PlacedProp,
    placed_by_rule: &mut BTreeMap<RuleKey, Vec<PlacedProp>>,
    placed_props: &mut Vec<PlannedProp>,
    required_keys: &mut BTreeSet<LocalPlacementKey>,
) {
    let key = placement_key(rule.key, copy);
    // Attachments sit on their host's surface. Their xy offset already
    // centers them on the host, so only the surface height is added here.
    let offset_m = match rule.strategy {
        PlacementStrategy::Attachment { host, .. } => Vec3::new(
            placed.offset.0,
            placed.offset.1,
            host_height(placed_by_rule, host),
        ),
        _ => Vec3::new(placed.offset.0, placed.offset.1, 0.0),
    };
    placed_props.push(PlannedProp {
        key,
        prop: placed.prop,
        anchor: placed.anchor,
        facing: placed.facing,
        room: Some(room.id),
        host: host_key(rule),
        offset_m,
        loot: rule.loot,
    });
    if placed.required {
        required_keys.insert(key);
    }
    placed_by_rule.entry(rule.key).or_default().push(placed);
}

fn host_height(placed_by_rule: &BTreeMap<RuleKey, Vec<PlacedProp>>, host: RuleKey) -> f32 {
    placed_by_rule
        .get(&host)
        .and_then(|props| props.first())
        .map_or(0.0, |host| host.prop.def().collision_max.z)
}

/// Readables authored on a fixture follow that fixture's anchor, facing and
/// surface height, so the `Read` prompt always targets a supported sign.
fn anchor_signs(blueprint: &mut BlueprintGrid) -> Result<(), FitError> {
    let props = blueprint.props.clone();
    for sign in &mut blueprint.signs {
        let Some(host) = sign.host else {
            continue;
        };
        let key = placement_key(host, 0);
        let Some(prop) = props.iter().find(|prop| prop.key == key) else {
            return Err(FitError::ImpossibleRoom {
                reason: "readable host fixture did not fit",
            });
        };
        sign.anchor = prop.anchor;
        sign.facing = prop.facing;
        sign.offset_m = Vec3::new(
            prop.offset_m.x,
            prop.offset_m.y,
            prop.prop.def().collision_max.z,
        );
    }
    Ok(())
}

fn placement_key(rule: RuleKey, copy: u8) -> LocalPlacementKey {
    LocalPlacementKey((rule.0 << 8) | u32::from(copy))
}

fn host_key(rule: &PropPlacementRule) -> Option<LocalPlacementKey> {
    match rule.strategy {
        PlacementStrategy::RelativeTo { parent, .. }
        | PlacementStrategy::Attachment { host: parent, .. }
        | PlacementStrategy::WallFacing { host: parent, .. }
        | PlacementStrategy::Adjacent { parent, .. } => Some(placement_key(parent, 0)),
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
    // Fixtures and their seating/support groups come before clutter. Within a
    // stage, parents precede children and higher priority wins ties.
    ordered.sort_by_key(|(rule, depth)| {
        (
            u8::from(rule.required == 0),
            u8::from(rule.clutter),
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
    let mut parents: Vec<RuleKey> = Vec::new();
    match rule.strategy {
        PlacementStrategy::RelativeTo { parent, .. }
        | PlacementStrategy::Attachment { host: parent, .. }
        | PlacementStrategy::WallFacing { host: parent, .. } => parents.push(parent),
        PlacementStrategy::Adjacent {
            parent, requires, ..
        } => {
            parents.push(parent);
            parents.extend(requires);
        }
        _ => {}
    }
    let mut max_depth = 0u8;
    for parent in parents {
        let Some(parent_rule) = keys.get(&parent) else {
            return Err(FitError::ImpossibleRoom {
                reason: "furnishing rule references a missing parent",
            });
        };
        let parent_depth = dependency_depth(parent_rule, keys, depth + 1)?;
        max_depth = max_depth.max(parent_depth.saturating_add(1));
    }
    Ok(max_depth)
}

/// Clearance a rule keeps from unrelated furniture. Required fixtures check
/// their approach instead, and intentional relationships such as kitchen
/// cabinet runs or countertop attachments are exempt by construction.
fn rule_clearance_m(room: RoomKind, rule: &PropPlacementRule) -> f32 {
    if rule.required > 0 {
        return 0.0;
    }
    match rule.strategy {
        PlacementStrategy::FloorOpen { .. } => CLUTTER_CLEARANCE_M,
        PlacementStrategy::WallRun { .. }
        | PlacementStrategy::WallFacing { .. }
        | PlacementStrategy::Adjacent { .. }
            if room == RoomKind::Living =>
        {
            CLUTTER_CLEARANCE_M
        }
        _ => 0.0,
    }
}

/// The parent a relational rule is anchored to. Its placements are related by
/// design, so they are excluded from the clearance check.
fn relation_parent(rule: &PropPlacementRule) -> Option<RuleKey> {
    match rule.strategy {
        PlacementStrategy::RelativeTo { parent, .. }
        | PlacementStrategy::Attachment { host: parent, .. }
        | PlacementStrategy::WallFacing { host: parent, .. }
        | PlacementStrategy::Adjacent { parent, .. } => Some(parent),
        _ => None,
    }
}

fn unrelated_placements(
    placed_by_rule: &BTreeMap<RuleKey, Vec<PlacedProp>>,
    rule: &PropPlacementRule,
) -> Vec<PlacedProp> {
    let parent = relation_parent(rule);
    placed_by_rule
        .iter()
        .filter(|(key, _)| Some(**key) != parent)
        .flat_map(|(_, props)| props.iter().copied())
        .collect()
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
    let clearance = rule_clearance_m(room.kind, rule);
    let existing = unrelated_placements(placed_by_rule, rule);
    let required = copy < rule.required;
    for candidate in candidates {
        if !candidate_fits(blueprint, room_cells, reserved, occupancy, &candidate) {
            continue;
        }
        let placed = candidate.placed(required);
        if clearance > 0.0 && !has_prop_clearance(&placed, &existing, clearance) {
            continue;
        }
        let def = candidate.prop.def();
        let cells = footprint(def.span, candidate.facing, candidate.anchor);
        // Trial state includes the candidate's own footprint, so its approach
        // path can never run through the fixture it serves.
        let mut trial_reserved = reserved.clone();
        let mut trial_occupancy = occupancy.clone();
        trial_occupancy.insert(&cells, def.attachment);
        if def.attachment == CellAttachment::Floor {
            trial_occupancy.insert_physical(placed_rect(&placed), candidate.anchor.floor);
        }
        if required
            && def.attachment == CellAttachment::Floor
            && !reserve_interaction_approach(
                blueprint,
                room_cells,
                &mut trial_reserved,
                &trial_occupancy,
                &cells,
                candidate.facing,
            )
        {
            continue;
        }
        if def.attachment == CellAttachment::Floor
            && !circulation_kept(
                room_cells,
                reserved,
                occupancy,
                &trial_reserved,
                &trial_occupancy,
                &cells,
            )
        {
            continue;
        }
        *reserved = trial_reserved;
        *occupancy = trial_occupancy;
        return Ok(Some(placed));
    }
    if required {
        return Err(FitError::ImpossibleRoom {
            reason: "required furnishing fixture did not fit",
        });
    }
    Ok(None)
}

/// Whether a floor placement leaves every currently reachable free tile of
/// the room reachable from circulation. Walks from reserved room tiles over
/// free room tiles, before and after the trial placement. Tiles the candidate
/// itself covers may be lost; any other tile must stay connected, so a bed
/// cannot wall off a nightstand pocket or a sofa seal a doorway.
fn circulation_kept(
    room_cells: &HashSet<LocalCell>,
    reserved_before: &HashSet<LocalCell>,
    occupancy_before: &Occupancy,
    reserved_after: &HashSet<LocalCell>,
    occupancy_after: &Occupancy,
    candidate_cells: &[LocalCell],
) -> bool {
    let before = reachable_free_tiles(room_cells, reserved_before, occupancy_before);
    if before.is_empty() {
        return true;
    }
    let after = reachable_free_tiles(room_cells, reserved_after, occupancy_after);
    before
        .iter()
        .all(|cell| candidate_cells.contains(cell) || after.contains(cell))
}

fn reachable_free_tiles(
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
) -> HashSet<LocalCell> {
    let free = |cell: &LocalCell| room_cells.contains(cell) && !occupancy.floor.contains(cell);
    let mut seen: HashSet<LocalCell> = HashSet::new();
    let mut queue: VecDeque<LocalCell> = reserved.iter().copied().filter(free).collect();
    for &cell in &queue {
        seen.insert(cell);
    }
    while let Some(cell) = queue.pop_front() {
        for (du, dv) in NEIGHBORS {
            let next = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
            if free(&next) && seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    seen
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

/// Whether a candidate's tiles, window sightlines and physical collider are
/// free. Attachments are checked on their layer only; they sit on a host.
fn candidate_fits(
    blueprint: &BlueprintGrid,
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
    candidate: &Candidate,
) -> bool {
    let def = candidate.prop.def();
    let cells = footprint(def.span, candidate.facing, candidate.anchor);
    if cells.is_empty()
        || !cells.iter().all(|cell| room_cells.contains(cell))
        || occupancy.blocks(&cells, def.attachment)
    {
        return false;
    }
    if def.attachment == CellAttachment::Floor && cells.iter().any(|c| reserved.contains(c)) {
        return false;
    }
    if cells
        .iter()
        .any(|cell| below_is_window(blueprint, *cell, def.cover))
    {
        return false;
    }
    if def.attachment == CellAttachment::Floor {
        let rect = prop_rect(
            candidate.prop,
            candidate.anchor,
            candidate.facing,
            candidate.offset,
        );
        let floor = candidate.anchor.floor;
        if occupancy.physical_overlap(rect, floor)
            || rect_hits_walls(blueprint, rect, floor)
            || rect_hits_reserved(rect, reserved, floor)
        {
            return false;
        }
    }
    true
}

/// Physical clearance check. Returns `false` when `candidate` would leave less
/// than `min_gap_m` of open floor to any floor fixture in `existing`.
fn has_prop_clearance(candidate: &PlacedProp, existing: &[PlacedProp], min_gap_m: f32) -> bool {
    if min_gap_m <= 0.0 {
        return true;
    }
    let rect = placed_rect(candidate);
    existing
        .iter()
        .filter(|prop| {
            prop.anchor.floor == candidate.anchor.floor
                && prop.prop.def().attachment == CellAttachment::Floor
        })
        .all(|prop| rect_gap(rect, placed_rect(prop)) >= min_gap_m - EPS)
}

/// Edge-to-edge distance between two rectangles, zero when they touch.
fn rect_gap(a: Rect, b: Rect) -> f32 {
    let du = (a.u0 - b.u1).max(b.u0 - a.u1).max(0.0);
    let dv = (a.v0 - b.v1).max(b.v0 - a.v1).max(0.0);
    (du * du + dv * dv).sqrt()
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
) -> Vec<Candidate> {
    let mut out = Vec::new();
    for &prop in rule.alternatives {
        let def = prop.def();
        let mut candidates: Vec<Candidate> = match rule.strategy {
            PlacementStrategy::WallRun { preference, .. } => {
                wall_run_candidates(blueprint, room, def, preference)
                    .into_iter()
                    .filter_map(|(anchor, facing)| {
                        aligned_candidate(blueprint, prop, anchor, facing)
                    })
                    .collect()
            }
            PlacementStrategy::WallFacing { host, preference } => {
                let Some(host) = placed_by_rule.get(&host).and_then(|props| props.first()) else {
                    continue;
                };
                let host_center = placed_rect(host).center();
                wall_run_candidates(blueprint, room, def, preference)
                    .into_iter()
                    .filter_map(|(anchor, facing)| {
                        aligned_candidate(blueprint, prop, anchor, facing)
                    })
                    .filter(|candidate| faces_point(candidate, host_center))
                    .collect()
            }
            PlacementStrategy::FloorOpen {
                min_wall_clearance, ..
            } => plain_candidates(
                prop,
                floor_open_candidates(
                    blueprint,
                    room,
                    room_cells,
                    reserved,
                    occupancy,
                    def,
                    min_wall_clearance,
                ),
            ),
            PlacementStrategy::AisleRun { clear_gap, .. } => plain_candidates(
                prop,
                floor_open_candidates(
                    blueprint, room, room_cells, reserved, occupancy, def, clear_gap,
                ),
            ),
            PlacementStrategy::RelativeTo { parent, offsets } => plain_candidates(
                prop,
                relative_candidates(
                    room_cells,
                    reserved,
                    occupancy,
                    placed_by_rule,
                    parent,
                    offsets,
                ),
            ),
            PlacementStrategy::Attachment { layer, host } => attachment_candidates(
                prop,
                room_cells,
                reserved,
                occupancy,
                placed_by_rule,
                host,
                layer,
            ),
            PlacementStrategy::Adjacent {
                parent,
                side,
                gap_m,
                align,
                face,
                requires,
            } => {
                if requires.is_some_and(|key| placed_by_rule.get(&key).is_none_or(Vec::is_empty)) {
                    continue;
                }
                let Some(parent) = placed_by_rule.get(&parent).and_then(|props| props.first())
                else {
                    continue;
                };
                adjacent_candidates(prop, parent, side, gap_m, align, face)
            }
        };
        candidates.retain(|candidate| {
            let cells = footprint(def.span, candidate.facing, candidate.anchor);
            !cells.is_empty()
                && cells.iter().all(|cell| room_cells.contains(cell))
                && !occupancy.blocks(&cells, def.attachment)
                && (def.attachment != CellAttachment::Floor
                    || !cells.iter().any(|cell| reserved.contains(cell)))
        });
        match rule.strategy {
            PlacementStrategy::WallRun { preference, .. }
            | PlacementStrategy::WallFacing { preference, .. } => {
                sort_wall_candidates(seed, blueprint, room, def, preference, &mut candidates);
            }
            PlacementStrategy::FloorOpen { prefer_corner, .. } => {
                let center = room_center(room);
                candidates.sort_by_key(|candidate| {
                    let anchor = candidate.anchor;
                    let score = if prefer_corner {
                        corner_distance(room, anchor)
                    } else {
                        (anchor.u - center.0).abs() + (anchor.v - center.1).abs()
                    };
                    (score, anchor.v, anchor.u)
                });
            }
            _ => {}
        }
        out.extend(candidates);
    }
    out
}

fn plain_candidates(prop: PropId, list: Vec<(BlueprintAnchor, PropFacing)>) -> Vec<Candidate> {
    list.into_iter()
        .map(|(anchor, facing)| Candidate {
            prop,
            anchor,
            facing,
            offset: (0.0, 0.0),
        })
        .collect()
}

fn sort_wall_candidates(
    seed: u64,
    blueprint: &BlueprintGrid,
    room: &RoomMeta,
    def: crate::action::PropDef,
    preference: WallPreference,
    candidates: &mut [Candidate],
) {
    candidates.sort_by_key(|candidate| {
        let cells = footprint(def.span, candidate.facing, candidate.anchor);
        let (window_rank, run) = back_wall_score(blueprint, &cells, candidate.facing, preference);
        let center = room_center(room);
        let (au, av) = (candidate.anchor.u, candidate.anchor.v);
        (
            u8::from(preference == WallPreference::WindowThenAny && window_rank == 0),
            u32::MAX - run,
            (au - center.0).abs() + (av - center.1).abs(),
            mix64(seed ^ FURNISH_SALT ^ u64::from(au as u32) << 8 ^ u64::from(av as u32)),
            av,
            au,
        )
    });
}

/// Physical gap (meters) a wall-backed fixture keeps from its wall. These are
/// the midpoints of the design ranges for each fixture family.
fn wall_gap_m(prop: PropId) -> f32 {
    match prop {
        PropId::Bed | PropId::BedDouble => 0.04,
        PropId::Bathtub => 0.05,
        PropId::Toilet => 0.06,
        PropId::BathroomSink => 0.05,
        PropId::Sofa => 0.10,
        PropId::BookcaseOpen | PropId::BookcaseClosed => 0.05,
        PropId::KitchenSink
        | PropId::KitchenStove
        | PropId::KitchenCabinet
        | PropId::Fridge
        | PropId::TvStand
        | PropId::Counter => 0.04,
        _ => 0.05,
    }
}

fn aligned_candidate(
    blueprint: &BlueprintGrid,
    prop: PropId,
    anchor: BlueprintAnchor,
    facing: PropFacing,
) -> Option<Candidate> {
    let offset = wall_alignment_offset(blueprint, prop, anchor, facing, wall_gap_m(prop))?;
    Some(Candidate {
        prop,
        anchor,
        facing,
        offset,
    })
}

/// Translation (meters, blueprint `(u, v)`) that moves a wall-backed fixture so
/// its physical back face sits `gap_m` from the interior face of the wall
/// behind it. The formula is `n * (wall_face - back_face - gap)`, with `n`
/// pointing toward the wall and both faces projected onto `n`. The wall face
/// uses the same thickness as the runtime wall colliders.
fn wall_alignment_offset(
    blueprint: &BlueprintGrid,
    prop: PropId,
    anchor: BlueprintAnchor,
    facing: PropFacing,
    gap_m: f32,
) -> Option<(f32, f32)> {
    let cells = footprint(prop.def().span, facing, anchor);
    let (nu, nv) = back_normal(facing);
    let edge = *back_edge(&cells, facing).first()?;
    let wall = LocalCell::new(edge.u + nu, edge.v + nv, edge.floor);
    if !blueprint
        .cell(wall)
        .is_some_and(|cell| is_wall_like(cell.kind))
    {
        return None;
    }
    let (nuf, nvf) = (nu as f32, nv as f32);
    let wall_face = (wall.u as f32 * nuf + wall.v as f32 * nvf) - WALL_HALF_THICKNESS;
    let rect = prop_rect(prop, anchor, facing, (0.0, 0.0));
    let (cu, cv) = rect.center();
    let half_along_n = if nu != 0 {
        (rect.u1 - rect.u0) * 0.5
    } else {
        (rect.v1 - rect.v0) * 0.5
    };
    let back_face = cu * nuf + cv * nvf + half_along_n;
    let shift = wall_face - back_face - gap_m;
    Some((nuf * shift, nvf * shift))
}

/// Whether a candidate's center lies in front of `point` within a roughly
/// 45° cone around its facing. Used by `WallFacing` so a TV stand faces its
/// sofa across the room.
fn faces_point(candidate: &Candidate, point: (f32, f32)) -> bool {
    let (cu, cv) = candidate_rect(candidate).center();
    let (du, dv) = (point.0 - cu, point.1 - cv);
    let len = (du * du + dv * dv).sqrt();
    if len < 2.0 {
        return false;
    }
    let (fu, fv) = facing_offset(candidate.facing);
    (du * fu as f32 + dv * fv as f32) / len >= 0.7
}

/// Physical placement beside a parent's rotated bounds. Each side and gap in
/// the range produces a candidate, and both tile anchors around the exact
/// center are offered so the physical position is not forced to a tile grid.
fn adjacent_candidates(
    prop: PropId,
    parent: &PlacedProp,
    side: ParentSide,
    gap_m: (f32, f32),
    align: Align,
    face: ChildFacing,
) -> Vec<Candidate> {
    let parent_rect = placed_rect(parent);
    let (pcu, pcv) = parent_rect.center();
    let (phu, phv) = half_size(parent_rect);
    let f = facing_offset(parent.facing);
    let left = (-f.1, f.0);
    let right = (f.1, -f.0);
    let back = (-f.0, -f.1);
    let dirs: Vec<(i32, i32)> = match side {
        ParentSide::Front => vec![f],
        ParentSide::Back => vec![back],
        ParentSide::Left => vec![left],
        ParentSide::Right => vec![right],
        ParentSide::Sides => vec![left, right],
        ParentSide::Any => vec![f, back, left, right],
    };
    let mut out = Vec::new();
    for dir in dirs {
        for gap in gap_samples(gap_m) {
            let child_facing = match face {
                ChildFacing::Toward => facing_from_offset((-dir.0, -dir.1)),
                ChildFacing::Parent => parent.facing,
            };
            let (cols, rows) = footprint_dims(prop.def().span, child_facing);
            let (chu, chv) = half_extents(prop, child_facing);
            let parent_extent = if dir.0 != 0 { phu } else { phv };
            let child_extent = if dir.0 != 0 { chu } else { chv };
            let distance = parent_extent + gap + child_extent;
            let mut cu = pcu + dir.0 as f32 * distance;
            let mut cv = pcv + dir.1 as f32 * distance;
            if align == Align::Back {
                // Flush the child's back face with the parent's back face along
                // the parent's facing axis.
                let (fu, fv) = (f.0 as f32, f.1 as f32);
                let parent_depth = if f.0 != 0 { phu } else { phv };
                let child_depth = if f.0 != 0 { chu } else { chv };
                let target = pcu * fu + pcv * fv - parent_depth + child_depth;
                let shift = target - (cu * fu + cv * fv);
                cu += fu * shift;
                cv += fv * shift;
            }
            let (bu, bv) = ((cols as f32 - 1.0) * 0.5, (rows as f32 - 1.0) * 0.5);
            for au in anchors_around(cu - bu) {
                for av in anchors_around(cv - bv) {
                    let anchor = BlueprintAnchor::new(au, av, parent.anchor.floor);
                    out.push(Candidate {
                        prop,
                        anchor,
                        facing: child_facing,
                        offset: (cu - (au as f32 + bu), cv - (av as f32 + bv)),
                    });
                }
            }
        }
    }
    out
}

fn gap_samples((min, max): (f32, f32)) -> Vec<f32> {
    let mid = (min + max) * 0.5;
    let mut out = vec![min];
    for value in [mid, max] {
        if out.iter().all(|existing| (existing - value).abs() > 1e-4) {
            out.push(value);
        }
    }
    out
}

fn anchors_around(value: f32) -> Vec<i32> {
    let lo = value.floor() as i32;
    let hi = value.ceil() as i32;
    if lo == hi { vec![lo] } else { vec![lo, hi] }
}

fn room_center(room: &RoomMeta) -> (i32, i32) {
    let (min_u, max_u, min_v, max_v) = room_bbox(room);
    ((min_u + max_u) / 2, (min_v + max_v) / 2)
}

fn corner_distance(room: &RoomMeta, anchor: BlueprintAnchor) -> i32 {
    let (min_u, max_u, min_v, max_v) = room_bbox(room);
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

/// Footprint dimensions `(u, v)` in tiles for a facing. E/W swap the span.
fn footprint_dims(span: PropSpan, facing: PropFacing) -> (i32, i32) {
    if matches!(facing, PropFacing::East | PropFacing::West) {
        (span.height as i32, span.width as i32)
    } else {
        (span.width as i32, span.height as i32)
    }
}

fn footprint(span: PropSpan, facing: PropFacing, anchor: BlueprintAnchor) -> Vec<LocalCell> {
    let (cols, rows) = footprint_dims(span, facing);
    (0..rows)
        .flat_map(|v| {
            (0..cols).map(move |u| LocalCell::new(anchor.u + u, anchor.v + v, anchor.floor))
        })
        .collect()
}

/// Half extents `(u, v)` of the collider in blueprint space. Matches the
/// runtime prop collider: depth is the facing axis.
fn half_extents(prop: PropId, facing: PropFacing) -> (f32, f32) {
    let def = prop.def();
    if matches!(facing, PropFacing::East | PropFacing::West) {
        (def.collision_max.y, def.collision_max.x)
    } else {
        (def.collision_max.x, def.collision_max.y)
    }
}

fn half_size(rect: Rect) -> (f32, f32) {
    ((rect.u1 - rect.u0) * 0.5, (rect.v1 - rect.v0) * 0.5)
}

/// Pivot of the tile span plus the stored physical offset, as an unrounded
/// rectangle in blueprint meters.
fn prop_rect(
    prop: PropId,
    anchor: BlueprintAnchor,
    facing: PropFacing,
    offset: (f32, f32),
) -> Rect {
    let (cols, rows) = footprint_dims(prop.def().span, facing);
    let cu = anchor.u as f32 + (cols as f32 - 1.0) * 0.5 + offset.0;
    let cv = anchor.v as f32 + (rows as f32 - 1.0) * 0.5 + offset.1;
    let (hu, hv) = half_extents(prop, facing);
    Rect {
        u0: cu - hu,
        u1: cu + hu,
        v0: cv - hv,
        v1: cv + hv,
    }
}

fn placed_rect(prop: &PlacedProp) -> Rect {
    prop_rect(prop.prop, prop.anchor, prop.facing, prop.offset)
}

fn candidate_rect(candidate: &Candidate) -> Rect {
    prop_rect(
        candidate.prop,
        candidate.anchor,
        candidate.facing,
        candidate.offset,
    )
}

fn is_wall_like(kind: BlueprintKind) -> bool {
    kind.is_wall() || matches!(kind, BlueprintKind::Door { .. })
}

fn wall_at(blueprint: &BlueprintGrid, u: i32, v: i32, floor: Floor) -> bool {
    blueprint
        .cell(LocalCell::new(u, v, floor))
        .is_some_and(|cell| is_wall_like(cell.kind))
}

/// Thin collider rectangles for the wall-like cell at `cell`, mirroring the
/// runtime geometry: a straight run is a full meter long and
/// `WALL_HALF_THICKNESS` thick. Corners reach only their connected arms.
fn wall_bars(blueprint: &BlueprintGrid, cell: LocalCell) -> Vec<Rect> {
    let Some(here) = blueprint.cell(cell) else {
        return Vec::new();
    };
    if !is_wall_like(here.kind) {
        return Vec::new();
    }
    let (u, v) = (cell.u as f32, cell.v as f32);
    let t = WALL_HALF_THICKNESS;
    let w = wall_at(blueprint, cell.u - 1, cell.v, cell.floor);
    let e = wall_at(blueprint, cell.u + 1, cell.v, cell.floor);
    let n = wall_at(blueprint, cell.u, cell.v - 1, cell.floor);
    let s = wall_at(blueprint, cell.u, cell.v + 1, cell.floor);
    let horizontal = w || e;
    let vertical = n || s;
    if !horizontal && !vertical {
        return vec![Rect {
            u0: u - 0.12,
            u1: u + 0.12,
            v0: v - 0.12,
            v1: v + 0.12,
        }];
    }
    let mut bars = Vec::with_capacity(2);
    if horizontal {
        let (a0, a1) = if vertical {
            (if w { 0.5 } else { t }, if e { 0.5 } else { t })
        } else {
            (0.5, 0.5)
        };
        bars.push(Rect {
            u0: u - a0,
            u1: u + a1,
            v0: v - t,
            v1: v + t,
        });
    }
    if vertical {
        let (a0, a1) = if horizontal {
            (if n { 0.5 } else { t }, if s { 0.5 } else { t })
        } else {
            (0.5, 0.5)
        };
        bars.push(Rect {
            u0: u - t,
            u1: u + t,
            v0: v - a0,
            v1: v + a1,
        });
    }
    bars
}

/// Tile range covered by a physical rectangle, padded by one tile so every
/// wall bar that can reach it is visited.
fn tile_range(rect: Rect) -> (std::ops::RangeInclusive<i32>, std::ops::RangeInclusive<i32>) {
    (
        (rect.u0 - 1.0).floor() as i32..=(rect.u1 + 1.0).ceil() as i32,
        (rect.v0 - 1.0).floor() as i32..=(rect.v1 + 1.0).ceil() as i32,
    )
}

fn rect_hits_walls(blueprint: &BlueprintGrid, rect: Rect, floor: Floor) -> bool {
    let (us, vs) = tile_range(rect);
    for u in us {
        for v in vs.clone() {
            for bar in wall_bars(blueprint, LocalCell::new(u, v, floor)) {
                if rect.overlaps(bar) {
                    return true;
                }
            }
        }
    }
    false
}

fn rect_hits_reserved(rect: Rect, reserved: &HashSet<LocalCell>, floor: Floor) -> bool {
    let (us, vs) = tile_range(rect);
    for u in us {
        for v in vs.clone() {
            if reserved.contains(&LocalCell::new(u, v, floor)) && rect.overlaps(tile_rect(u, v)) {
                return true;
            }
        }
    }
    false
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
        let (cols, rows) = footprint_dims(def.span, facing);
        for v in min_v..=max_v - rows + 1 {
            for u in min_u..=max_u - cols + 1 {
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

fn facing_from_offset(offset: (i32, i32)) -> PropFacing {
    match offset {
        (0, -1) => PropFacing::North,
        (1, 0) => PropFacing::East,
        (0, 1) => PropFacing::South,
        _ => PropFacing::West,
    }
}

fn floor_open_candidates(
    blueprint: &BlueprintGrid,
    room: &RoomMeta,
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
    def: crate::action::PropDef,
    min_wall_clearance: u8,
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
        let (cols, rows) = footprint_dims(def.span, facing);
        for v in min_v..=max_v - rows + 1 {
            for u in min_u..=max_u - cols + 1 {
                let anchor = BlueprintAnchor::new(u, v, floor);
                let cells = footprint(def.span, facing, anchor);
                if cells.is_empty()
                    || !cells.iter().all(|cell| room_cells.contains(cell))
                    || occupancy.blocks(&cells, def.attachment)
                    || (def.attachment == CellAttachment::Floor
                        && cells.iter().any(|cell| reserved.contains(cell)))
                    || !wall_clearance_ok(blueprint, &cells, min_wall_clearance)
                {
                    continue;
                }
                out.push((anchor, facing));
            }
        }
    }
    out
}

/// Keep `k` tiles of open floor between a floor-open fixture and any wall.
fn wall_clearance_ok(blueprint: &BlueprintGrid, cells: &[LocalCell], k: u8) -> bool {
    if k == 0 {
        return true;
    }
    let k = i32::from(k);
    cells.iter().all(|cell| {
        (-k..=k).all(|du| {
            (-k..=k).all(|dv| {
                let near = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
                cells.contains(&near) || !wall_at(blueprint, near.u, near.v, near.floor)
            })
        })
    })
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

/// A layer-attached fixture sits centered on its host's footprint, using the
/// host's physical pivot and offset. Its xy offset places it in the middle of
/// wider hosts (a TV on a two-tile stand).
fn attachment_candidates(
    prop: PropId,
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
    placed_by_rule: &BTreeMap<RuleKey, Vec<PlacedProp>>,
    host: RuleKey,
    layer: CellAttachment,
) -> Vec<Candidate> {
    let Some(host) = placed_by_rule.get(&host).and_then(|props| props.first()) else {
        return Vec::new();
    };
    let cells = footprint(host.prop.def().span, host.facing, host.anchor);
    if !cells.iter().all(|cell| room_cells.contains(cell))
        || occupancy.blocks(&cells, layer)
        || cells.iter().any(|cell| reserved.contains(cell))
    {
        return Vec::new();
    }
    let (hu, hv) = {
        let (cols, rows) = footprint_dims(host.prop.def().span, host.facing);
        (
            host.anchor.u as f32 + (cols as f32 - 1.0) * 0.5 + host.offset.0,
            host.anchor.v as f32 + (rows as f32 - 1.0) * 0.5 + host.offset.1,
        )
    };
    let (pu, pv) = {
        let (cols, rows) = footprint_dims(prop.def().span, host.facing);
        (
            host.anchor.u as f32 + (cols as f32 - 1.0) * 0.5,
            host.anchor.v as f32 + (rows as f32 - 1.0) * 0.5,
        )
    };
    vec![Candidate {
        prop,
        anchor: host.anchor,
        facing: host.facing,
        offset: (hu - pu, hv - pv),
    }]
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

/// Outcome of one bathroom arrangement search.
struct GroupArrangement {
    placed: Vec<PlacedProp>,
    reserved: HashSet<LocalCell>,
    occupancy: Occupancy,
    walls: usize,
}

/// Bounded depth-first search over a required fixture group. Each branch
/// commits occupancy and approach reservations and rolls them back on
/// rejection. The best arrangement uses the fewest distinct walls, with ties
/// resolved by candidate order.
struct GroupSearch<'a> {
    blueprint: &'a BlueprintGrid,
    room_cells: &'a HashSet<LocalCell>,
    candidates: Vec<Vec<Candidate>>,
    budget: u32,
    best: Option<GroupArrangement>,
    chosen: Vec<PlacedProp>,
}

impl GroupSearch<'_> {
    fn descend(
        &mut self,
        level: usize,
        reserved: &mut HashSet<LocalCell>,
        occupancy: &mut Occupancy,
    ) {
        if level == self.candidates.len() {
            self.record(reserved, occupancy);
            return;
        }
        for index in 0..self.candidates[level].len() {
            if self.budget == 0 {
                return;
            }
            self.budget -= 1;
            let candidate = self.candidates[level][index];
            if !candidate_fits(
                self.blueprint,
                self.room_cells,
                reserved,
                occupancy,
                &candidate,
            ) {
                continue;
            }
            let def = candidate.prop.def();
            let cells = footprint(def.span, candidate.facing, candidate.anchor);
            let saved_reserved = reserved.clone();
            let saved_occupancy = occupancy.clone();
            if !reserve_interaction_approach(
                self.blueprint,
                self.room_cells,
                reserved,
                occupancy,
                &cells,
                candidate.facing,
            ) {
                *reserved = saved_reserved;
                continue;
            }
            occupancy.insert(&cells, def.attachment);
            occupancy.insert_physical(candidate_rect(&candidate), candidate.anchor.floor);
            self.chosen.push(candidate.placed(true));
            self.descend(level + 1, reserved, occupancy);
            self.chosen.pop();
            *reserved = saved_reserved;
            *occupancy = saved_occupancy;
        }
    }

    fn record(&mut self, reserved: &HashSet<LocalCell>, occupancy: &Occupancy) {
        let walls = self
            .chosen
            .iter()
            .map(wall_identity)
            .collect::<BTreeSet<_>>()
            .len();
        if self.best.as_ref().is_some_and(|best| best.walls <= walls) {
            return;
        }
        self.best = Some(GroupArrangement {
            placed: self.chosen.clone(),
            reserved: reserved.clone(),
            occupancy: occupancy.clone(),
            walls,
        });
    }
}

/// Identity of the wall a fixture backs onto: normal plus the wall's line.
fn wall_identity(prop: &PlacedProp) -> (i32, i32, i32) {
    let (nu, nv) = back_normal(prop.facing);
    let cells = footprint(prop.prop.def().span, prop.facing, prop.anchor);
    let Some(edge) = back_edge(&cells, prop.facing).first().copied() else {
        return (nu, nv, 0);
    };
    let (wu, wv) = (edge.u + nu, edge.v + nv);
    (nu, nv, if nu != 0 { wu } else { wv })
}

/// Place a required fixture group together. The search commits the best
/// arrangement under [`GROUP_SEARCH_BUDGET`]; a rejected group leaves the
/// occupancy and reservations exactly as they were.
#[allow(clippy::too_many_arguments)]
fn place_group(
    seed: u64,
    blueprint: &BlueprintGrid,
    room: &RoomMeta,
    room_cells: &HashSet<LocalCell>,
    reserved: &mut HashSet<LocalCell>,
    occupancy: &mut Occupancy,
    placed_by_rule: &BTreeMap<RuleKey, Vec<PlacedProp>>,
    rules: &[&PropPlacementRule],
) -> Result<Vec<PlacedProp>, FitError> {
    let mut candidates = Vec::with_capacity(rules.len());
    for rule in rules {
        let list = candidates_for(
            seed,
            blueprint,
            room,
            room_cells,
            reserved,
            occupancy,
            placed_by_rule,
            rule,
        );
        if list.is_empty() {
            return Err(FitError::ImpossibleRoom {
                reason: "bathroom fixture has no wall-aligned candidate",
            });
        }
        candidates.push(list);
    }
    let mut search = GroupSearch {
        blueprint,
        room_cells,
        candidates,
        budget: GROUP_SEARCH_BUDGET,
        best: None,
        chosen: Vec::new(),
    };
    let mut work_reserved = reserved.clone();
    let mut work_occupancy = occupancy.clone();
    search.descend(0, &mut work_reserved, &mut work_occupancy);
    match search.best.take() {
        Some(best) => {
            *reserved = best.reserved;
            *occupancy = best.occupancy;
            Ok(best.placed)
        }
        None if search.budget == 0 => Err(FitError::BudgetExhausted),
        None => Err(FitError::ImpossibleRoom {
            reason: "bathroom fixtures have no reachable arrangement",
        }),
    }
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

fn group_rules_for(kind: RoomKind) -> &'static [RuleKey] {
    match kind {
        RoomKind::Bathroom => BATHROOM_GROUP,
        _ => &[],
    }
}

const CHAIR_SIDES_OF_TABLE: ParentSide = ParentSide::Any;

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
        clutter: false,
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
        clutter: false,
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
        clutter: false,
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
        clutter: false,
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
        clutter: true,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(15),
        alternatives: &[PropId::Chair],
        strategy: PlacementStrategy::Adjacent {
            parent: RuleKey(14),
            side: CHAIR_SIDES_OF_TABLE,
            gap_m: (0.05, 0.15),
            align: Align::Center,
            face: ChildFacing::Toward,
            requires: None,
        },
        required: 0,
        maximum: 4,
        extra_chance: 1.0,
        priority: 20,
        clutter: true,
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
        clutter: false,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(21),
        alternatives: &[PropId::TvStand],
        strategy: PlacementStrategy::WallFacing {
            host: RuleKey(20),
            preference: WallPreference::Any,
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 70,
        clutter: false,
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
        clutter: false,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(23),
        alternatives: &[PropId::TableCoffee],
        strategy: PlacementStrategy::Adjacent {
            parent: RuleKey(20),
            side: ParentSide::Front,
            gap_m: (0.35, 0.60),
            align: Align::Center,
            face: ChildFacing::Toward,
            requires: Some(RuleKey(21)),
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 50,
        clutter: false,
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
        clutter: true,
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
        clutter: false,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(31),
        alternatives: &[PropId::Nightstand],
        strategy: PlacementStrategy::Adjacent {
            parent: RuleKey(30),
            side: ParentSide::Sides,
            gap_m: (0.03, 0.12),
            align: Align::Back,
            face: ChildFacing::Parent,
            requires: None,
        },
        required: 1,
        maximum: 2,
        extra_chance: 0.8,
        priority: 60,
        clutter: false,
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
        clutter: true,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(33),
        alternatives: &[PropId::ChairDesk],
        strategy: PlacementStrategy::Adjacent {
            parent: RuleKey(32),
            side: ParentSide::Front,
            gap_m: (0.05, 0.15),
            align: Align::Center,
            face: ChildFacing::Toward,
            requires: None,
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 40,
        clutter: true,
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
        clutter: false,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(41),
        alternatives: &[PropId::Toilet],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Any,
            segments: (1, 4),
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 90,
        clutter: false,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(42),
        alternatives: &[PropId::BathroomSink],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Any,
            segments: (1, 4),
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 80,
        clutter: false,
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
        clutter: true,
        loot: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Position;
    use crate::ReadableId;
    use crate::world::archetypes::houses::craftsman_blueprint;
    use crate::world::blueprint::{CardinalDir, Frontage};

    const SIDES: [CardinalDir; 4] = [
        CardinalDir::North,
        CardinalDir::East,
        CardinalDir::South,
        CardinalDir::West,
    ];
    /// Wall-contact tolerance for fixtures authored against a wall.
    const WALL_GAP_MAX_M: f32 = 0.10 + EPS;
    /// Bed-to-nightstand gap range authored in `Adjacent`.
    const NIGHTSTAND_GAP_M: (f32, f32) = (0.03 - EPS, 0.12 + EPS);
    /// Floor-open clutter clearance (`CLUTTER_CLEARANCE_M`) with tolerance.
    const CLEARANCE_M: f32 = CLUTTER_CLEARANCE_M - EPS;

    fn frontage(side: CardinalDir) -> Frontage {
        Frontage {
            side,
            access: Position::new(115, 100),
        }
    }

    fn furnished(seed: u64, side: CardinalDir) -> BlueprintGrid {
        craftsman_blueprint(seed, frontage(side)).expect("the Craftsman furnishes")
    }

    fn planned_rect(prop: &PlannedProp) -> Rect {
        prop_rect(
            prop.prop,
            prop.anchor,
            prop.facing,
            (prop.offset_m.x, prop.offset_m.y),
        )
    }

    fn wall_rects(blueprint: &BlueprintGrid) -> Vec<Rect> {
        blueprint
            .cells
            .keys()
            .flat_map(|cell| wall_bars(blueprint, *cell))
            .collect()
    }

    /// Smallest separation between `rect` and a wall bar that overlaps it on
    /// one axis. Diagonal corners are ignored, so this is the gap to the wall
    /// the fixture stands against.
    fn wall_face_gap(rect: Rect, walls: &[Rect]) -> f32 {
        let mut best = f32::INFINITY;
        for wall in walls {
            if wall.v0 < rect.v1 && rect.v0 < wall.v1 {
                let gap = (wall.u0 - rect.u1).max(rect.u0 - wall.u1);
                if gap >= -EPS {
                    best = best.min(gap.max(0.0));
                }
            }
            if wall.u0 < rect.u1 && rect.u0 < wall.u1 {
                let gap = (wall.v0 - rect.v1).max(rect.v0 - wall.v1);
                if gap >= -EPS {
                    best = best.min(gap.max(0.0));
                }
            }
        }
        best
    }

    #[test]
    fn wall_fixtures_sit_within_gap_of_a_wall() {
        let kinds = [
            PropId::BedDouble,
            PropId::Bed,
            PropId::Bathtub,
            PropId::Toilet,
            PropId::BathroomSink,
            PropId::Sofa,
        ];
        for seed in 0..8 {
            for side in SIDES {
                let bp = furnished(seed, side);
                let walls = wall_rects(&bp);
                for prop in bp.props.iter().filter(|p| kinds.contains(&p.prop)) {
                    let gap = wall_face_gap(planned_rect(prop), &walls);
                    assert!(
                        gap <= WALL_GAP_MAX_M,
                        "seed {seed} {side:?}: {:?} at {:?} is {gap:.3} m from a wall",
                        prop.prop,
                        prop.anchor
                    );
                }
            }
        }
    }

    #[test]
    fn nightstand_sits_beside_bed_head_and_hosts_the_note() {
        let nightstand_key = placement_key(RuleKey(31), 0);
        for seed in 0..8 {
            for side in SIDES {
                let bp = furnished(seed, side);
                let bed = bp
                    .props
                    .iter()
                    .find(|p| matches!(p.prop, PropId::BedDouble | PropId::Bed))
                    .expect("bed");
                let stand = bp
                    .props
                    .iter()
                    .find(|p| p.key == nightstand_key)
                    .expect("primary nightstand");
                assert_eq!(stand.prop, PropId::Nightstand);
                let gap = rect_gap(planned_rect(bed), planned_rect(stand));
                assert!(
                    gap >= NIGHTSTAND_GAP_M.0 && gap <= NIGHTSTAND_GAP_M.1,
                    "seed {seed} {side:?}: nightstand gap {gap:.3} m"
                );
                let note = bp
                    .signs
                    .iter()
                    .find(|s| s.readable == Some(ReadableId::CraftsmanNote))
                    .expect("note");
                assert_eq!(note.host, Some(RuleKey(31)));
                assert_eq!(
                    (note.anchor.u, note.anchor.v, note.anchor.floor),
                    (stand.anchor.u, stand.anchor.v, stand.anchor.floor),
                    "note must follow the nightstand anchor"
                );
            }
        }
    }

    #[test]
    fn television_sits_on_its_stand_against_a_wall() {
        for seed in 0..8 {
            for side in SIDES {
                let bp = furnished(seed, side);
                let stand = bp
                    .props
                    .iter()
                    .find(|p| p.prop == PropId::TvStand)
                    .expect("tv stand");
                let tv = bp
                    .props
                    .iter()
                    .find(|p| p.prop == PropId::Television)
                    .expect("television");
                assert_eq!(tv.host, Some(stand.key), "TV must host on the stand");
                assert_eq!(tv.anchor, stand.anchor);
                let gap = wall_face_gap(planned_rect(stand), &wall_rects(&bp));
                assert!(
                    gap <= WALL_GAP_MAX_M,
                    "seed {seed} {side:?}: TV stand {gap:.3} m from a wall"
                );
            }
        }
    }

    #[test]
    fn craftsman_tall_furniture_fits_beneath_ceiling() {
        assert!(PropId::BookcaseOpen.def().collision_max.z < crate::spatial::WALL_HEIGHT);
        assert!(PropId::Fridge.def().collision_max.z < crate::spatial::WALL_HEIGHT);
        assert!(
            (0..64).any(|seed| SIDES.into_iter().any(|side| {
                furnished(seed, side)
                    .props
                    .iter()
                    .any(|p| p.prop == PropId::BookcaseOpen)
            })),
            "bookcases should remain eligible for placement"
        );
    }

    #[test]
    fn dining_table_keeps_clearance_from_unrelated_furniture() {
        let mut tables = 0;
        for seed in 0..64 {
            for side in SIDES {
                let bp = furnished(seed, side);
                for table in bp.props.iter().filter(|p| p.prop == PropId::Table) {
                    tables += 1;
                    let table_key = table.key;
                    for other in bp.props.iter().filter(|p| {
                        p.key != table_key
                            && p.host != Some(table_key)
                            && p.anchor.floor == table.anchor.floor
                            && p.prop.def().attachment == CellAttachment::Floor
                    }) {
                        let gap = rect_gap(planned_rect(table), planned_rect(other));
                        assert!(
                            gap >= CLEARANCE_M,
                            "seed {seed} {side:?}: table {:.3} m from {:?}",
                            gap,
                            other.prop
                        );
                    }
                }
            }
        }
        assert!(tables > 0, "the seed sweep should place at least one table");
    }

    #[test]
    fn clearance_check_rejects_crowding_and_accepts_open_floor() {
        let table = PlacedProp {
            prop: PropId::Table,
            anchor: BlueprintAnchor::new(4, 4, 0),
            facing: PropFacing::North,
            required: false,
            offset: (0.0, 0.0),
        };
        let near = PlacedProp {
            prop: PropId::BookcaseOpen,
            anchor: BlueprintAnchor::new(6, 4, 0),
            facing: PropFacing::North,
            required: false,
            offset: (-0.5, 0.0),
        };
        let far = PlacedProp {
            anchor: BlueprintAnchor::new(9, 4, 0),
            ..near
        };
        let near_gap = rect_gap(placed_rect(&table), placed_rect(&near));
        let far_gap = rect_gap(placed_rect(&table), placed_rect(&far));
        assert!(near_gap < CLUTTER_CLEARANCE_M, "fixture crowds the table");
        assert!(
            far_gap >= CLUTTER_CLEARANCE_M,
            "far fixture leaves a walkway"
        );
        assert!(!has_prop_clearance(&near, &[table], CLUTTER_CLEARANCE_M));
        assert!(has_prop_clearance(&far, &[table], CLUTTER_CLEARANCE_M));
        assert!(has_prop_clearance(&near, &[table], 0.0));
    }

    #[test]
    fn craftsman_furnishes_for_representative_seed_sweep() {
        for seed in 0..64 {
            for side in SIDES {
                craftsman_blueprint(seed, frontage(side))
                    .unwrap_or_else(|err| panic!("seed {seed} {side:?}: {err:?}"));
            }
        }
    }
}
