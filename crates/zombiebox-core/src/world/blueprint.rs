//! Sparse, authored building blueprints and their lot-local transform.
//!
//! Phase 3 generation authors a structure once in local `(u, v, floor)` space.
//! `u` runs along the street frontage and `v` runs into the lot. A
//! [`LotTransform`] places that blueprint in the county with a cardinal basis,
//! so every downstream consumer (world tiles, the action stage, props and
//! signs) sees a single canonical world-space description.

use std::collections::BTreeMap;

use crate::action::{
    BuildingStyle, CellAttachment, DoorAxis, Material, PropAnchor, PropFacing, PropId, PropSpan,
    ReadableId, Surface, Tile3D, TileKind, Vec3, mix64,
};
use crate::{Floor, Position};

use super::RoomKind;

/// Inclusive world-tile rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridRect {
    pub min: Position,
    pub max: Position,
}

impl GridRect {
    pub const fn new(min: Position, max: Position) -> Self {
        Self { min, max }
    }

    pub const fn width(self) -> i32 {
        self.max.x - self.min.x + 1
    }

    pub const fn height(self) -> i32 {
        self.max.y - self.min.y + 1
    }

    pub fn contains(self, p: Position) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

/// A stable, seeded parcel identity derived from canonical world coordinates.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ParcelId(pub u64);

/// A stable identity for one structure slot inside a parcel.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StructureId(pub u64);

/// Authored placement key, unique inside one structure.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalPlacementKey(pub u32);

/// Stable identity for one placed prop. This is the `PropAnchor::anchor_uid`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlacementId(pub u64);

pub type RoomId = u16;
pub type WingId = u8;

const PARCEL_NAMESPACE: u64 = 0x5a42_5033_5041_5243; // "ZBP3PARC"
const STRUCTURE_NAMESPACE: u64 = 0x5a42_5033_5354_5255; // "ZBP3STRU"
const PLACEMENT_NAMESPACE: u64 = 0x5a42_5033_5052_4f50; // "ZBP3PROP"

fn pack_position(p: Position) -> u64 {
    ((u32::from_ne_bytes(p.x.to_ne_bytes()) as u64) << 32)
        | u32::from_ne_bytes(p.y.to_ne_bytes()) as u64
}

pub fn parcel_id(seed: u64, bounds: GridRect) -> ParcelId {
    ParcelId(
        mix64(
            seed ^ PARCEL_NAMESPACE
                ^ pack_position(bounds.min)
                ^ pack_position(bounds.max).rotate_left(17),
        ) | 1,
    )
}

pub fn structure_id(parcel: ParcelId, slot: u16) -> StructureId {
    StructureId(mix64(parcel.0 ^ STRUCTURE_NAMESPACE ^ (u64::from(slot) << 1)).max(1))
}

pub fn placement_id(structure: StructureId, key: LocalPlacementKey) -> PlacementId {
    PlacementId(mix64(structure.0 ^ PLACEMENT_NAMESPACE ^ (u64::from(key.0) << 1)).max(1))
}

/// Cardinal side of the lot that carries the public frontage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardinalDir {
    North,
    East,
    South,
    West,
}

impl CardinalDir {
    /// Unit direction into the lot from this frontage.
    pub const fn inward(self) -> (i32, i32) {
        match self {
            Self::North => (0, 1),
            Self::East => (-1, 0),
            Self::South => (0, -1),
            Self::West => (1, 0),
        }
    }

    /// Unit direction along the frontage from local `u = 0` toward increasing `u`.
    pub const fn right(self) -> (i32, i32) {
        match self {
            Self::North => (-1, 0),
            Self::East => (0, -1),
            Self::South => (1, 0),
            Self::West => (0, 1),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frontage {
    pub side: CardinalDir,
    /// World tile on the lot boundary where the public approach enters.
    pub access: Position,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LotTransform {
    /// World tile corresponding to local `(0, 0)`.
    pub origin: Position,
    pub frontage: CardinalDir,
}

impl LotTransform {
    pub const fn new(origin: Position, frontage: CardinalDir) -> Self {
        Self { origin, frontage }
    }

    pub const fn inward(self) -> (i32, i32) {
        self.frontage.inward()
    }

    pub const fn right(self) -> (i32, i32) {
        self.frontage.right()
    }

    pub fn local_to_world(self, local: LocalCell) -> Position {
        let right = self.right();
        let inward = self.inward();
        Position::new(
            self.origin.x + local.u * right.0 + local.v * inward.0,
            self.origin.y + local.u * right.1 + local.v * inward.1,
        )
    }

    pub fn world_axis(self, local: DoorAxis) -> DoorAxis {
        let tangent = match local {
            DoorAxis::AlongX => self.right(),
            DoorAxis::AlongY => self.inward(),
        };
        if tangent.0 != 0 {
            DoorAxis::AlongX
        } else {
            DoorAxis::AlongY
        }
    }

    pub fn world_stair_dir(self, local: (i8, i8)) -> (i8, i8) {
        let right = self.right();
        let inward = self.inward();
        let x = i32::from(local.0) * right.0 + i32::from(local.1) * inward.0;
        let y = i32::from(local.0) * right.1 + i32::from(local.1) * inward.1;
        (x as i8, y as i8)
    }

    pub fn world_facing(self, local: PropFacing) -> PropFacing {
        let (du, dv) = match local {
            PropFacing::North => (0, -1),
            PropFacing::East => (1, 0),
            PropFacing::South => (0, 1),
            PropFacing::West => (-1, 0),
        };
        let right = self.right();
        let inward = self.inward();
        let x = du * right.0 + dv * inward.0;
        let y = du * right.1 + dv * inward.1;
        match (x, y) {
            (0, -1) => PropFacing::North,
            (1, 0) => PropFacing::East,
            (0, 1) => PropFacing::South,
            _ => PropFacing::West,
        }
    }

    pub fn world_offset(self, local: Vec3) -> Vec3 {
        let right = self.right();
        let inward = self.inward();
        Vec3::new(
            local.x * right.0 as f32 + local.y * inward.0 as f32,
            local.x * right.1 as f32 + local.y * inward.1 as f32,
            local.z,
        )
    }
}

/// A local tile plus floor index. `u` and `v` are blueprint-space coordinates;
/// `v = 0` is the frontage wall and increasing `v` runs into the lot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalCell {
    pub u: i32,
    pub v: i32,
    pub floor: Floor,
}

impl LocalCell {
    pub const fn new(u: i32, v: i32, floor: Floor) -> Self {
        Self { u, v, floor }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlueprintKind {
    Empty,
    Floor,
    ExteriorWall,
    InteriorWall,
    Pillar,
    Window,
    Door { exterior: bool, gate: bool },
    GarageDoor { segment: u8 },
    Stairs { up: bool, down: bool },
}

impl BlueprintKind {
    pub fn is_wall(self) -> bool {
        matches!(
            self,
            Self::ExteriorWall | Self::InteriorWall | Self::Pillar | Self::Window
        )
    }

    pub fn walkable(self) -> bool {
        matches!(
            self,
            Self::Floor | Self::Door { .. } | Self::GarageDoor { .. } | Self::Stairs { .. }
        )
    }

    pub fn to_feature(self, material: Material) -> super::Feature {
        match self {
            Self::Empty => super::Feature::Empty,
            Self::Floor => super::Feature::Floor,
            Self::ExteriorWall | Self::InteriorWall => super::Feature::Wall(material),
            Self::Pillar => super::Feature::Wall(Material::MetalPole),
            Self::Window => super::Feature::Window,
            Self::Door { gate: true, .. } => super::Feature::Gate,
            Self::GarageDoor { .. } => super::Feature::Gate,
            Self::Door { .. } => super::Feature::Door,
            Self::Stairs { .. } => super::Feature::Stairs,
        }
    }

    pub fn to_tile_kind(self) -> TileKind {
        match self {
            Self::Empty | Self::Floor => TileKind::Empty,
            Self::ExteriorWall => TileKind::ExteriorWall,
            Self::InteriorWall => TileKind::InteriorWall,
            Self::Pillar => TileKind::Pillar,
            Self::Window => TileKind::Window,
            Self::Door { exterior, .. } => TileKind::Door {
                open: false,
                exterior,
            },
            Self::GarageDoor { segment } => TileKind::GarageDoor {
                open: false,
                segment,
            },
            Self::Stairs {
                up: true,
                down: true,
            } => TileKind::StairsBoth,
            Self::Stairs {
                up: true,
                down: false,
            } => TileKind::StairsUp,
            Self::Stairs {
                up: false,
                down: true,
            } => TileKind::StairsDown,
            Self::Stairs {
                up: false,
                down: false,
            } => TileKind::Empty,
        }
    }
}

/// Per-cell semantic data. Cells are sparse; absent cells are open sky.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlueprintCell {
    pub kind: BlueprintKind,
    pub room: Option<RoomId>,
    pub wing: WingId,
    pub material: Material,
    pub surface: Surface,
    pub indoor: bool,
    pub load_bearing: bool,
    pub roof: bool,
    pub stair_dir: (i8, i8),
}

impl BlueprintCell {
    pub const fn new(kind: BlueprintKind, material: Material, surface: Surface) -> Self {
        Self {
            kind,
            room: None,
            wing: 0,
            material,
            surface,
            indoor: false,
            load_bearing: false,
            roof: false,
            stair_dir: (0, 0),
        }
    }

    pub fn with_room(mut self, room: RoomId) -> Self {
        self.room = Some(room);
        self
    }

    pub fn indoor(mut self) -> Self {
        self.indoor = true;
        self
    }

    pub fn load_bearing(mut self) -> Self {
        self.load_bearing = true;
        self
    }

    pub fn with_roof(mut self) -> Self {
        self.roof = true;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoorKind {
    Exterior,
    Interior,
    Gate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoorIntent {
    pub cell: LocalCell,
    /// Wall tangent in blueprint space. The runtime derives the normal.
    pub tangent: DoorAxis,
    pub kind: DoorKind,
    pub connects: (SpaceId, SpaceId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpaceId {
    Room(RoomId),
    Exterior,
    Unit(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StairLink {
    pub lower: LocalCell,
    pub upper: LocalCell,
    /// A clear landing cell adjacent to each endpoint.
    pub lower_landing: LocalCell,
    pub upper_landing: LocalCell,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoomMeta {
    pub id: RoomId,
    pub kind: RoomKind,
    pub unit: Option<u8>,
    pub wing: WingId,
    pub floor: Floor,
    /// The room's authoritative covered-cell mask.
    pub cells: Vec<LocalCell>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WallPreference {
    Any,
    Interior,
    Exterior,
    /// The fixture requires a window in its back wall.
    Window,
    /// Prefer a window, but accept any solid wall.
    WindowThenAny,
}

/// Stable authored rule identity inside one room kind.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuleKey(pub u32);

/// Offset from a placed parent rule. `facing` orients the child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalPropOffset {
    pub du: i32,
    pub dv: i32,
    pub facing: PropFacing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlacementStrategy {
    /// Back against a matching wall. The physical back face is then pushed to
    /// the wall with a small gap (see `furnish::wall_alignment_offset`).
    WallRun {
        preference: WallPreference,
        segments: (u8, u8),
    },
    /// Like `WallRun`, but only walls whose facing points toward the placed
    /// `host` across the room. Used for a TV stand opposite a sofa.
    WallFacing {
        host: RuleKey,
        preference: WallPreference,
    },
    AisleRun {
        clear_gap: u8,
        end_margin: u8,
        endcap: Option<PropId>,
    },
    /// Legacy tile-offset placement from a parent's anchor.
    RelativeTo {
        parent: RuleKey,
        offsets: &'static [LocalPropOffset],
    },
    /// Physical placement beside a parent's rotated bounds, with a gap range.
    /// `requires` gates the rule on another placed rule (no candidates until
    /// that rule has a placed instance).
    Adjacent {
        parent: RuleKey,
        side: ParentSide,
        gap_m: (f32, f32),
        align: Align,
        face: ChildFacing,
        requires: Option<RuleKey>,
    },
    FloorOpen {
        min_wall_clearance: u8,
        prefer_corner: bool,
    },
    Attachment {
        layer: CellAttachment,
        host: RuleKey,
    },
}

/// Which side of a parent's rotated bounds an `Adjacent` child occupies.
/// Sides are relative to the parent's facing (`Front` is its facing direction).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParentSide {
    Front,
    Back,
    Left,
    Right,
    /// Left or right, whichever is tried first.
    Sides,
    Any,
}

/// Lateral alignment of an `Adjacent` child against its parent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Align {
    /// Centered on the parent along the shared edge.
    Center,
    /// Back face flush with the parent's back face.
    Back,
}

/// Orientation of an `Adjacent` child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildFacing {
    /// Face the parent (chairs, coffee table).
    Toward,
    /// Share the parent's facing (nightstands beside a bed).
    Parent,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LootProfileId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlannedProp {
    pub key: LocalPlacementKey,
    pub prop: PropId,
    pub anchor: BlueprintAnchor,
    pub facing: PropFacing,
    pub room: Option<RoomId>,
    pub host: Option<LocalPlacementKey>,
    pub offset_m: Vec3,
    pub loot: Option<LootProfileId>,
}

/// Anchor in blueprint space. The lower/left covered tile is canonical.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlueprintAnchor {
    pub u: i32,
    pub v: i32,
    pub floor: Floor,
}

impl BlueprintAnchor {
    pub const fn new(u: i32, v: i32, floor: Floor) -> Self {
        Self { u, v, floor }
    }

    pub const fn local(self) -> LocalCell {
        LocalCell::new(self.u, self.v, self.floor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlannedSign {
    pub key: LocalPlacementKey,
    pub anchor: BlueprintAnchor,
    pub prop: PropId,
    pub attachment: CellAttachment,
    pub offset_m: Vec3,
    pub facing: PropFacing,
    pub readable: Option<ReadableId>,
    /// When set, furnishing re-anchors the sign to this rule's first placed
    /// prop (keeping its facing and surface height) instead of `anchor`.
    pub host: Option<RuleKey>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VehicleSlot {
    pub anchor: BlueprintAnchor,
    pub facing: PropFacing,
    pub length: u8,
    pub width: u8,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutdoorLayer {
    pub paths: Vec<(Position, Position)>,
    /// Vehicle routes are separate from pedestrian approaches.
    pub driveways: Vec<(Position, Position)>,
    pub fences: Vec<(Position, Position)>,
    pub props: Vec<(PropId, Position, PropFacing)>,
    pub signs: Vec<(Position, PropFacing)>,
    pub vehicle_slots: Vec<(Position, PropFacing)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneKind {
    Residential,
    Commercial,
    Civic,
    Service,
    Rural,
}

/// A parcel's immutable plan. Phase 3.1 only consumes the blueprint structure
/// fields; the outdoor layer is retained for later packages.
#[derive(Clone, Debug, PartialEq)]
pub struct ParcelPlan {
    pub id: ParcelId,
    pub bounds: GridRect,
    pub frontage: Frontage,
    pub zone: ZoneKind,
    pub structures: Vec<PlacedStructure>,
    pub outdoor: OutdoorLayer,
}

impl ParcelPlan {
    pub fn structures_intersecting(
        &self,
        bounds: GridRect,
    ) -> impl Iterator<Item = &PlacedStructure> {
        self.structures
            .iter()
            .filter(move |structure| structure.bounds.intersects(bounds))
    }
}

/// Sparse authored structure placed in world space.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedStructure {
    pub id: StructureId,
    pub name: &'static str,
    pub archetype: &'static str,
    pub style: BuildingStyle,
    pub transform: LotTransform,
    pub foundation_z: f32,
    pub blueprint: BlueprintGrid,
    pub bounds: GridRect,
    pub front_door: Position,
}

impl PlacedStructure {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        parcel: ParcelId,
        slot: u16,
        name: &'static str,
        archetype: &'static str,
        style: BuildingStyle,
        transform: LotTransform,
        blueprint: BlueprintGrid,
    ) -> Self {
        let bounds = blueprint.world_bounds(transform);
        let front_door = blueprint
            .doors
            .iter()
            .filter(|door| matches!(door.kind, DoorKind::Exterior | DoorKind::Gate))
            .min_by_key(|door| (door.cell.floor, door.cell.v, door.cell.u))
            .map(|door| transform.local_to_world(door.cell))
            .unwrap_or(Position::new(bounds.min.x, bounds.min.y));
        Self {
            id: structure_id(parcel, slot),
            name,
            archetype,
            style,
            transform,
            foundation_z: 0.0,
            blueprint,
            bounds,
            front_door,
        }
    }

    pub const fn floors(&self) -> u8 {
        self.blueprint.floors
    }

    pub fn door_axis_at(&self, local: LocalCell) -> Option<DoorAxis> {
        self.blueprint
            .doors
            .iter()
            .find(|door| door.cell == local)
            .map(|door| self.transform.world_axis(door.tangent))
            .or_else(|| {
                let BlueprintKind::GarageDoor { .. } = self.blueprint.cell(local)?.kind else {
                    return None;
                };
                let dirs = [(1, 0), (-1, 0), (0, 1), (0, -1)];
                dirs.into_iter().find_map(|(du, dv)| {
                    let center = LocalCell::new(local.u + du, local.v + dv, local.floor);
                    (matches!(self.blueprint.cell(center)?.kind, BlueprintKind::GarageDoor { segment: 1 })
                        && self.blueprint.doors.iter().any(|door| door.cell == center))
                    .then(|| self.blueprint.doors.iter().find(|door| door.cell == center)
                        .map(|door| self.transform.world_axis(door.tangent)))
                    .flatten()
                })
            })
    }

    pub fn cell_at_world(
        &self,
        pos: Position,
        floor: Floor,
    ) -> Option<(&BlueprintCell, Option<DoorAxis>)> {
        let local = self.blueprint.world_to_local(self.transform, pos, floor)?;
        let cell = self.blueprint.cells.get(&local)?;
        Some((cell, self.door_axis_at(local)))
    }

    /// Iterate every world-space cell once, in canonical local order.
    pub fn world_cells(&self) -> impl Iterator<Item = (LocalCell, Position, &BlueprintCell)> {
        self.blueprint
            .cells
            .iter()
            .map(move |(&local, cell)| (local, self.transform.local_to_world(local), cell))
    }

    /// Stable world-space prop anchors. Multi-tile footprints are transformed
    /// as covered cells, then reduced to the world min corner and span.
    pub fn props_world(&self) -> Vec<BlueprintPropPlacement> {
        let mut props = Vec::with_capacity(self.blueprint.props.len());
        for prop in &self.blueprint.props {
            let def = prop.prop.def();
            let local_cells = prop.local_covered_cells(def.span);
            let mut min = Position::new(i32::MAX, i32::MAX);
            let mut max = Position::new(i32::MIN, i32::MIN);
            for local in local_cells {
                let world = self.transform.local_to_world(local);
                min = Position::new(min.x.min(world.x), min.y.min(world.y));
                max = Position::new(max.x.max(world.x), max.y.max(world.y));
            }
            let facing = self.transform.world_facing(prop.facing);
            let span = world_span_for_facing(def.span, facing);
            debug_assert_eq!(
                span,
                PropSpan::new((max.x - min.x + 1) as u8, (max.y - min.y + 1) as u8)
            );
            props.push(BlueprintPropPlacement {
                structure: self.id,
                anchor_uid: placement_id(self.id, prop.key).0,
                tile: Tile3D::new(min.x, min.y, prop.anchor.floor),
                id: prop.prop,
                span,
                attachment: def.attachment,
                facing,
                offset: self.transform.world_offset(prop.offset_m),
                room: prop.room,
                loot: prop.loot,
            });
        }
        props
    }

    pub fn planned_props(&self) -> Vec<PropAnchor> {
        self.props_world()
            .into_iter()
            .map(BlueprintPropPlacement::to_anchor)
            .collect()
    }

    /// Stable world-space readables and wall/floor signs authored in the
    /// blueprint. Signs use their own placement keys so a readable cannot
    /// collide with a furniture anchor.
    pub fn signs_world(&self) -> Vec<BlueprintSignPlacement> {
        self.blueprint
            .signs
            .iter()
            .map(|sign| {
                let world = self.transform.local_to_world(sign.anchor.local());
                BlueprintSignPlacement {
                    structure: self.id,
                    readable: sign.readable,
                    anchor: PropAnchor {
                        anchor_uid: placement_id(self.id, sign.key).0,
                        tile: Tile3D::new(world.x, world.y, sign.anchor.floor),
                        id: sign.prop,
                        span: PropSpan::new(1, 1),
                        attachment: sign.attachment,
                        facing: self.transform.world_facing(sign.facing),
                        offset: self.transform.world_offset(sign.offset_m),
                    },
                }
            })
            .collect()
    }

    pub fn planned_signs(&self) -> Vec<PropAnchor> {
        self.signs_world()
            .into_iter()
            .map(|sign| sign.anchor)
            .collect()
    }

    /// Set the final foundation after relief. Pads and world bounds are already
    /// final at this point; only the elevation changes.
    pub fn set_foundation(&mut self, foundation_z: f32) {
        self.foundation_z = foundation_z;
    }
}

pub(crate) fn world_span_for_facing(span: PropSpan, facing: PropFacing) -> PropSpan {
    if matches!(facing, PropFacing::East | PropFacing::West) {
        PropSpan::new(span.height, span.width)
    } else {
        span
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlueprintPropPlacement {
    pub structure: StructureId,
    pub anchor_uid: u64,
    pub tile: Tile3D,
    pub id: PropId,
    pub span: PropSpan,
    pub attachment: CellAttachment,
    pub facing: PropFacing,
    pub offset: Vec3,
    pub room: Option<RoomId>,
    pub loot: Option<LootProfileId>,
}

impl BlueprintPropPlacement {
    pub fn to_anchor(self) -> PropAnchor {
        PropAnchor {
            anchor_uid: self.anchor_uid,
            tile: self.tile,
            id: self.id,
            span: self.span,
            attachment: self.attachment,
            facing: self.facing,
            offset: self.offset,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlueprintSignPlacement {
    pub structure: StructureId,
    pub readable: Option<ReadableId>,
    pub anchor: PropAnchor,
}

impl PlannedProp {
    fn local_covered_cells(self, def_span: PropSpan) -> Vec<LocalCell> {
        let span = if matches!(self.facing, PropFacing::East | PropFacing::West) {
            PropSpan::new(def_span.height, def_span.width)
        } else {
            def_span
        };
        (0..span.height as i32)
            .flat_map(|y| {
                (0..span.width as i32).map(move |x| {
                    LocalCell::new(self.anchor.u + x, self.anchor.v + y, self.anchor.floor)
                })
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlueprintGrid {
    pub cells: BTreeMap<LocalCell, BlueprintCell>,
    pub rooms: Vec<RoomMeta>,
    pub doors: Vec<DoorIntent>,
    pub stairs: Vec<StairLink>,
    pub props: Vec<PlannedProp>,
    pub signs: Vec<PlannedSign>,
    pub vehicle_slots: Vec<VehicleSlot>,
    pub floors: u8,
}

impl BlueprintGrid {
    pub fn new(floors: u8) -> Self {
        Self {
            floors: floors.clamp(1, crate::MAX_COUNTY_FLOORS as u8),
            ..Self::default()
        }
    }

    pub fn set_cell(&mut self, local: LocalCell, cell: BlueprintCell) {
        self.cells.insert(local, cell);
    }

    pub fn cell(&self, local: LocalCell) -> Option<&BlueprintCell> {
        self.cells.get(&local)
    }

    pub fn add_door(&mut self, door: DoorIntent) {
        self.doors.push(door);
    }

    pub fn local_bounds(&self) -> GridRect {
        let mut min = Position::new(i32::MAX, i32::MAX);
        let mut max = Position::new(i32::MIN, i32::MIN);
        for key in self.cells.keys() {
            min = Position::new(min.x.min(key.u), min.y.min(key.v));
            max = Position::new(max.x.max(key.u), max.y.max(key.v));
        }
        GridRect::new(min, max)
    }

    pub fn world_bounds(&self, transform: LotTransform) -> GridRect {
        let local = self.local_bounds();
        let corners = [
            Position::new(local.min.x, local.min.y),
            Position::new(local.min.x, local.max.y),
            Position::new(local.max.x, local.min.y),
            Position::new(local.max.x, local.max.y),
        ];
        let mut min = Position::new(i32::MAX, i32::MAX);
        let mut max = Position::new(i32::MIN, i32::MIN);
        for corner in corners {
            let world = transform.local_to_world(LocalCell::new(corner.x, corner.y, 0));
            min = Position::new(min.x.min(world.x), min.y.min(world.y));
            max = Position::new(max.x.max(world.x), max.y.max(world.y));
        }
        GridRect::new(min, max)
    }

    pub fn world_to_local(
        &self,
        transform: LotTransform,
        pos: Position,
        floor: Floor,
    ) -> Option<LocalCell> {
        let right = transform.right();
        let inward = transform.inward();
        let dx = pos.x - transform.origin.x;
        let dy = pos.y - transform.origin.y;
        // The basis is orthonormal and cardinal, so the dot products are exact.
        let u = dx * right.0 + dy * right.1;
        let v = dx * inward.0 + dy * inward.1;
        let local = LocalCell::new(u, v, floor);
        if transform.local_to_world(local) == pos && self.cells.contains_key(&local) {
            Some(local)
        } else {
            None
        }
    }

    pub fn floors(&self) -> u8 {
        self.floors
    }

    /// Validate doorway stencils, support, stairs and room references. The
    /// fixture intentionally uses the same checks downstream packages rely on.
    pub fn validate(&self) -> Result<(), FitError> {
        if self.cells.is_empty() {
            return Err(FitError::ImpossibleRoom {
                reason: "blueprint has no cells",
            });
        }
        for (local, cell) in &self.cells {
            if local.floor >= self.floors {
                return Err(FitError::InvalidLot {
                    reason: "cell floor outside declared range",
                });
            }
            if cell.kind == BlueprintKind::Empty {
                return Err(FitError::ImpossibleRoom {
                    reason: "empty cells must be sparse",
                });
            }
        }
        for door in &self.doors {
            let Some(cell) = self.cells.get(&door.cell) else {
                return Err(FitError::AccessFailure {
                    reason: "door cell missing",
                });
            };
            if !matches!(
                cell.kind,
                BlueprintKind::Door { .. } | BlueprintKind::GarageDoor { .. }
            ) {
                return Err(FitError::AccessFailure {
                    reason: "door intent cell is not a door",
                });
            }
            if self
                .doors
                .iter()
                .filter(|other| other.cell == door.cell)
                .count()
                != 1
            {
                return Err(FitError::AccessFailure {
                    reason: "duplicate door intent",
                });
            }
            let garage_wide = matches!(cell.kind, BlueprintKind::GarageDoor { segment: 1 })
                && [-1, 1].into_iter().all(|sign| {
                    self.cells.get(&LocalCell::new(
                        door.cell.u + door.tangent.tangent().0 * sign,
                        door.cell.v + door.tangent.tangent().1 * sign,
                        door.cell.floor,
                    )).is_some_and(|adjacent| matches!(adjacent.kind, BlueprintKind::GarageDoor { .. }))
                });
            let tangent = door.tangent.tangent();
            let normal = door.tangent.normal();
            for sign in [-1, 1] {
                let jambs = self.cells.get(&LocalCell::new(
                    door.cell.u + tangent.0 * sign * if garage_wide { 2 } else { 1 },
                    door.cell.v + tangent.1 * sign * if garage_wide { 2 } else { 1 },
                    door.cell.floor,
                ));
                let inside = jambs.is_some();
                if inside && !jambs.is_some_and(|c| c.kind.is_wall()) {
                    return Err(FitError::AccessFailure {
                        reason: "door tangent jamb is not a wall",
                    });
                }
            }
            for sign in [-1, 1] {
                let approach = self.cells.get(&LocalCell::new(
                    door.cell.u + normal.0 * sign,
                    door.cell.v + normal.1 * sign,
                    door.cell.floor,
                ));
                if approach.is_some_and(|c| !c.kind.walkable()) {
                    return Err(FitError::AccessFailure {
                        reason: "door approach is blocked",
                    });
                }
            }
        }
        for local in self.cells.keys().filter(|local| local.floor > 0) {
            let below = LocalCell::new(local.u, local.v, local.floor - 1);
            if !self.cells.contains_key(&below) {
                return Err(FitError::SupportFailure {
                    reason: "upper floor cell has no supporting cell",
                });
            }
        }
        for stair in &self.stairs {
            if stair.lower.floor + 1 != stair.upper.floor {
                return Err(FitError::SupportFailure {
                    reason: "stair endpoints must be adjacent floors",
                });
            }
            if stair.lower.u != stair.upper.u || stair.lower.v != stair.upper.v {
                return Err(FitError::SupportFailure {
                    reason: "stair endpoints must align vertically",
                });
            }
            for (endpoint, up) in [(stair.lower, true), (stair.upper, false)] {
                let valid = self
                    .cells
                    .get(&endpoint)
                    .is_some_and(|cell| match cell.kind {
                        BlueprintKind::Stairs {
                            up: has_up,
                            down: has_down,
                        } => {
                            (if up { has_up } else { has_down })
                                && i16::from(cell.stair_dir.0).abs()
                                    + i16::from(cell.stair_dir.1).abs()
                                    == 1
                        }
                        _ => false,
                    });
                if !valid {
                    return Err(FitError::AccessFailure {
                        reason: "stair endpoint or direction invalid",
                    });
                }
            }
            for (endpoint, landing) in [
                (stair.lower, stair.lower_landing),
                (stair.upper, stair.upper_landing),
            ] {
                if landing.floor != endpoint.floor
                    || (landing.u - endpoint.u).abs() + (landing.v - endpoint.v).abs() != 1
                {
                    return Err(FitError::AccessFailure {
                        reason: "stair landing must be adjacent on the same floor",
                    });
                }
            }
            // Runtime uses the source stair's direction to choose the arrival
            // position on the other floor.
            for (endpoint, landing) in [
                (stair.lower, stair.lower_landing),
                (stair.upper, stair.upper_landing),
            ] {
                let direction = self.cells[&endpoint].stair_dir;
                if (landing.u - endpoint.u, landing.v - endpoint.v)
                    != (i32::from(direction.0), i32::from(direction.1))
                {
                    return Err(FitError::ImpossibleRoom {
                        reason: "stair direction does not point to its landing",
                    });
                }
            }
            for landing in [stair.lower_landing, stair.upper_landing] {
                if !self
                    .cells
                    .get(&landing)
                    .is_some_and(|cell| cell.kind.walkable())
                {
                    return Err(FitError::AccessFailure {
                        reason: "stair landing is not walkable",
                    });
                }
            }
        }
        for room in &self.rooms {
            if room.cells.is_empty() {
                return Err(FitError::ImpossibleRoom {
                    reason: "room has no covered cells",
                });
            }
            for local in &room.cells {
                if !self.cells.contains_key(local) {
                    return Err(FitError::ImpossibleRoom {
                        reason: "room references a missing cell",
                    });
                }
            }
        }
        for prop in &self.props {
            let Some(cell) = self.cells.get(&prop.anchor.local()) else {
                return Err(FitError::AccessFailure {
                    reason: "prop anchor is outside the blueprint",
                });
            };
            if !cell.kind.walkable() {
                return Err(FitError::AccessFailure {
                    reason: "prop anchor is not on walkable floor",
                });
            }
        }
        let mut keys = std::collections::BTreeSet::new();
        for prop in &self.props {
            if !keys.insert(prop.key) {
                return Err(FitError::ImpossibleRoom {
                    reason: "duplicate prop placement key",
                });
            }
        }
        for sign in &self.signs {
            if !keys.insert(sign.key) {
                return Err(FitError::ImpossibleRoom {
                    reason: "duplicate prop or sign placement key",
                });
            }
            let Some(cell) = self.cells.get(&sign.anchor.local()) else {
                return Err(FitError::AccessFailure {
                    reason: "sign anchor is outside the blueprint",
                });
            };
            if sign.attachment == CellAttachment::Floor && !cell.kind.walkable() {
                return Err(FitError::AccessFailure {
                    reason: "floor sign anchor is not walkable",
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FitError {
    InvalidLot { reason: &'static str },
    ImpossibleRoom { reason: &'static str },
    AccessFailure { reason: &'static str },
    SupportFailure { reason: &'static str },
    BudgetExhausted,
}

impl std::fmt::Display for FitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLot { reason }
            | Self::ImpossibleRoom { reason }
            | Self::AccessFailure { reason }
            | Self::SupportFailure { reason } => write!(f, "{reason}"),
            Self::BudgetExhausted => f.write_str("candidate budget exhausted"),
        }
    }
}

impl std::error::Error for FitError {}

/// Shared trait for later house and retail generators.
pub trait StructureGenerator {
    fn generate(&self, context: &StructureContext) -> Result<BlueprintGrid, FitError>;
}

/// Shared parcel-composition seam. Multi-structure parcels implement this and
/// return a single plan consumed by the common materializer.
pub trait CompoundPlanner {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError>;
}

/// Context passed to a [`CompoundPlanner`].
#[derive(Clone, Copy, Debug)]
pub struct ParcelContext {
    pub seed: u64,
    pub bounds: GridRect,
    pub frontage: Frontage,
    pub zone: ZoneKind,
}

#[derive(Clone, Copy, Debug)]
pub struct StructureContext {
    pub seed: u64,
    pub parcel: ParcelId,
    pub frontage: Frontage,
}

/// Small deterministic search that places an authored blueprint in a lot.
/// It prefers an origin where a multi-tile prop straddles a chunk seam so the
/// streaming acceptance fixture exercises seam ownership.
#[allow(clippy::too_many_arguments)]
pub fn place_blueprint_in_lot(
    seed: u64,
    parcel: ParcelId,
    lot: GridRect,
    frontage: Frontage,
    blueprint: BlueprintGrid,
    name: &'static str,
    archetype: &'static str,
    style: BuildingStyle,
) -> Result<PlacedStructure, FitError> {
    blueprint.validate()?;
    let local = blueprint.local_bounds();
    let base_transform = LotTransform::new(Position::new(0, 0), frontage.side);
    let mut rel_min = Position::new(i32::MAX, i32::MAX);
    let mut rel_max = Position::new(i32::MIN, i32::MIN);
    for corner in [
        Position::new(local.min.x, local.min.y),
        Position::new(local.min.x, local.max.y),
        Position::new(local.max.x, local.min.y),
        Position::new(local.max.x, local.max.y),
    ] {
        let world = base_transform.local_to_world(LocalCell::new(corner.x, corner.y, 0));
        rel_min = Position::new(rel_min.x.min(world.x), rel_min.y.min(world.y));
        rel_max = Position::new(rel_max.x.max(world.x), rel_max.y.max(world.y));
    }
    let world_w = rel_max.x - rel_min.x + 1;
    let world_h = rel_max.y - rel_min.y + 1;
    let margin = 2;
    let min_x = lot.min.x + margin;
    let max_x = lot.max.x - margin - world_w + 1;
    let min_y = lot.min.y + margin;
    let max_y = lot.max.y - margin - world_h + 1;
    if max_x < min_x || max_y < min_y || world_w <= 0 || world_h <= 0 {
        return Err(FitError::InvalidLot {
            reason: "blueprint does not fit inside the lot setback",
        });
    }

    let mut candidates = Vec::new();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let origin = Position::new(x - rel_min.x, y - rel_min.y);
            let transform = LotTransform::new(origin, frontage.side);
            let structure = PlacedStructure::new(
                parcel,
                0,
                name,
                archetype,
                style,
                transform,
                blueprint.clone(),
            );
            let crosses = structure.props_world().iter().any(|prop| {
                prop.span.covered_tiles(prop.tile).any(|tile| {
                    prop.span
                        .covered_tiles(prop.tile)
                        .any(|other| other.x >> 4 != tile.x >> 4 || other.y >> 4 != tile.y >> 4)
                })
            });
            candidates.push((crosses, structure));
        }
    }
    // Prefer a seam-crossing prop, then a stable seeded tie-break so the same
    // seed always selects the same authored origin.
    let best = candidates
        .into_iter()
        .min_by_key(|(crosses, structure)| {
            (
                u8::from(!*crosses),
                mix64(seed ^ pack_position(structure.transform.origin)),
                structure.transform.origin.y,
                structure.transform.origin.x,
            )
        })
        .map(|(_, structure)| structure)
        .ok_or(FitError::BudgetExhausted)?;
    Ok(best)
}

/// Place an authored structure against the public frontage rather than
/// searching the lot for a seam-crossing prop. The structure's local `v = 0`
/// edge sits one tile inside the lot frontage, and the lateral position gets a
/// small deterministic seed-based offset inside the remaining frontage.
#[allow(clippy::too_many_arguments)]
pub fn place_blueprint_along_frontage(
    seed: u64,
    parcel: ParcelId,
    lot: GridRect,
    frontage: Frontage,
    blueprint: BlueprintGrid,
    name: &'static str,
    archetype: &'static str,
    style: BuildingStyle,
) -> Result<PlacedStructure, FitError> {
    blueprint.validate()?;
    let rel = blueprint.world_bounds(LotTransform::new(Position::new(0, 0), frontage.side));
    let span_w = rel.width();
    let span_h = rel.height();
    const MARGIN: i32 = 1;
    const FRONTAGE_SALT: u64 = 0x4652_4f4e_5453_4554; // "FRONTSET"

    let (lateral_min, lateral_max, desired_min) = match frontage.side {
        CardinalDir::North => {
            let min_x = lot.min.x + MARGIN;
            let max_x = lot.max.x - MARGIN - span_w + 1;
            (min_x, max_x, Position::new(0, lot.min.y + MARGIN))
        }
        CardinalDir::South => {
            let min_x = lot.min.x + MARGIN;
            let max_x = lot.max.x - MARGIN - span_w + 1;
            (
                min_x,
                max_x,
                Position::new(0, lot.max.y - MARGIN - span_h + 1),
            )
        }
        CardinalDir::West => {
            let min_y = lot.min.y + MARGIN;
            let max_y = lot.max.y - MARGIN - span_h + 1;
            (min_y, max_y, Position::new(lot.min.x + MARGIN, 0))
        }
        CardinalDir::East => {
            let min_y = lot.min.y + MARGIN;
            let max_y = lot.max.y - MARGIN - span_h + 1;
            (
                min_y,
                max_y,
                Position::new(lot.max.x - MARGIN - span_w + 1, 0),
            )
        }
    };
    if lateral_max < lateral_min {
        return Err(FitError::InvalidLot {
            reason: "blueprint does not fit inside the frontage setback",
        });
    }
    let center = lateral_min + (lateral_max - lateral_min) / 2;
    let jitter = (mix64(seed ^ parcel.0 ^ FRONTAGE_SALT) % 5) as i32 - 2;
    let lateral = (center + jitter).clamp(lateral_min, lateral_max);
    let desired_min = match frontage.side {
        CardinalDir::North | CardinalDir::South => Position::new(lateral, desired_min.y),
        CardinalDir::West | CardinalDir::East => Position::new(desired_min.x, lateral),
    };
    if desired_min.x < lot.min.x
        || desired_min.y < lot.min.y
        || desired_min.x + span_w - 1 > lot.max.x
        || desired_min.y + span_h - 1 > lot.max.y
    {
        return Err(FitError::InvalidLot {
            reason: "blueprint does not fit inside the lot",
        });
    }
    let origin = Position::new(desired_min.x - rel.min.x, desired_min.y - rel.min.y);
    Ok(PlacedStructure::new(
        parcel,
        0,
        name,
        archetype,
        style,
        LotTransform::new(origin, frontage.side),
        blueprint,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_blueprint() -> BlueprintGrid {
        crate::world::archetypes::fixture_blueprint()
    }

    #[test]
    fn frontage_placement_rejects_insufficient_depth_in_all_orientations() {
        for side in [
            CardinalDir::North,
            CardinalDir::South,
            CardinalDir::East,
            CardinalDir::West,
        ] {
            let lot = match side {
                CardinalDir::North | CardinalDir::South => {
                    GridRect::new(Position::new(0, 0), Position::new(20, 4))
                }
                CardinalDir::East | CardinalDir::West => {
                    GridRect::new(Position::new(0, 0), Position::new(4, 20))
                }
            };
            assert!(
                place_blueprint_along_frontage(
                    1,
                    parcel_id(1, lot),
                    lot,
                    Frontage {
                        side,
                        access: lot.min
                    },
                    fixture_blueprint(),
                    "test",
                    "test",
                    BuildingStyle::WarmPutty
                )
                .is_err(),
                "{side:?}"
            );
        }
    }

    #[test]
    fn asymmetric_blueprint_transforms_in_all_four_orientations() {
        let bp = fixture_blueprint();
        bp.validate().unwrap();
        assert_eq!(
            bp.local_bounds(),
            GridRect::new(Position::new(0, 0), Position::new(6, 5))
        );
        for (side, expected_door_axis, expected_span) in [
            (CardinalDir::South, DoorAxis::AlongX, PropSpan::new(2, 1)),
            (CardinalDir::North, DoorAxis::AlongX, PropSpan::new(2, 1)),
            (CardinalDir::West, DoorAxis::AlongY, PropSpan::new(1, 2)),
            (CardinalDir::East, DoorAxis::AlongY, PropSpan::new(1, 2)),
        ] {
            let lot = GridRect::new(Position::new(100, 100), Position::new(140, 140));
            let structure = place_blueprint_in_lot(
                7,
                parcel_id(7, lot),
                lot,
                Frontage {
                    side,
                    access: Position::new(100, 100),
                },
                bp.clone(),
                "Fixture",
                "fixture",
                BuildingStyle::WarmPutty,
            )
            .unwrap();
            let bounds = structure.bounds;
            match side {
                CardinalDir::North | CardinalDir::South => {
                    assert_eq!(bounds.width(), 7);
                    assert_eq!(bounds.height(), 6);
                }
                CardinalDir::East | CardinalDir::West => {
                    assert_eq!(bounds.width(), 6);
                    assert_eq!(bounds.height(), 7);
                }
            }
            let door = structure.blueprint.doors[0].cell;
            let bounds = structure.bounds;
            let expected_door = match side {
                CardinalDir::South => Position::new(bounds.min.x + 2, bounds.max.y),
                CardinalDir::North => Position::new(bounds.max.x - 2, bounds.min.y),
                CardinalDir::West => Position::new(bounds.min.x, bounds.min.y + 2),
                CardinalDir::East => Position::new(bounds.max.x, bounds.max.y - 2),
            };
            assert_eq!(
                structure.transform.local_to_world(door),
                expected_door,
                "{side:?} door"
            );
            assert_eq!(structure.door_axis_at(door), Some(expected_door_axis));
            let props = structure.props_world();
            assert_eq!(props.len(), 1);
            assert_eq!(props[0].span, expected_span, "{side:?} span");
            let covered = props[0]
                .span
                .covered_tiles(props[0].tile)
                .collect::<Vec<_>>();
            assert_eq!(covered.len(), 2);
            // The pivot is derived from the world min corner and rotated span,
            // not from the unrotated authored anchor.
            let anchor = props[0].to_anchor();
            let expected_pivot = if matches!(side, CardinalDir::North | CardinalDir::South) {
                Vec3::new(anchor.tile.x as f32 + 0.5, anchor.tile.y as f32, 0.0)
            } else {
                Vec3::new(anchor.tile.x as f32, anchor.tile.y as f32 + 0.5, 0.0)
            };
            assert_eq!(anchor.pivot(0.0), expected_pivot);
        }
    }

    #[test]
    fn validation_rejects_blocked_and_unsupported_blueprints() {
        let mut bp = fixture_blueprint();
        bp.set_cell(
            LocalCell::new(2, 1, 0),
            BlueprintCell::new(BlueprintKind::InteriorWall, Material::Drywall, Surface::Air),
        );
        assert!(matches!(bp.validate(), Err(FitError::AccessFailure { .. })));

        let mut bp = fixture_blueprint();
        bp.set_cell(
            LocalCell::new(2, 6, 1),
            BlueprintCell::new(
                BlueprintKind::Floor,
                Material::WoodLight,
                Surface::WoodFloor,
            ),
        );
        bp.floors = 2;
        assert!(matches!(
            bp.validate(),
            Err(FitError::SupportFailure { .. })
        ));
    }

    #[test]
    fn optional_clutter_does_not_change_structure_openings_or_required_placement() {
        let base = fixture_blueprint();
        let mut with_clutter = base.clone();
        let required = with_clutter.props[0].key;
        let required_id = placement_id(StructureId(7), required);
        with_clutter.props.push(PlannedProp {
            key: LocalPlacementKey(99),
            prop: PropId::Crate,
            anchor: BlueprintAnchor::new(1, 1, 0),
            facing: PropFacing::East,
            room: Some(1),
            host: None,
            offset_m: Vec3::ZERO,
            loot: None,
        });
        assert_eq!(base.cells, with_clutter.cells);
        assert_eq!(base.doors, with_clutter.doors);
        assert_eq!(base.stairs, with_clutter.stairs);
        assert_eq!(
            placement_id(StructureId(7), with_clutter.props[0].key),
            required_id
        );
        assert_ne!(
            placement_id(StructureId(7), with_clutter.props[1].key),
            required_id
        );
    }

    #[test]
    fn placement_prefers_a_seam_crossing_even_span() {
        let bp = fixture_blueprint();
        let lot = GridRect::new(Position::new(96, 96), Position::new(136, 136));
        let structure = place_blueprint_in_lot(
            0,
            parcel_id(0, lot),
            lot,
            Frontage {
                side: CardinalDir::South,
                access: Position::new(96, 96),
            },
            bp,
            "Fixture",
            "fixture",
            BuildingStyle::WarmPutty,
        )
        .unwrap();
        let prop = structure.props_world()[0];
        let cells = prop.span.covered_tiles(prop.tile).collect::<Vec<_>>();
        assert!(cells.iter().any(|a| {
            cells
                .iter()
                .any(|b| a.x >> 4 != b.x >> 4 || a.y >> 4 != b.y >> 4)
        }));
    }
}
