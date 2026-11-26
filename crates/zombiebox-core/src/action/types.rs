//! Shared 2.5D types, world trait, input/HUD contracts, and gameplay events for `ActionGame`.

pub use crate::spatial::Vec2;
use crate::{District, Position};

pub const ACTION_TICKS_PER_SECOND: u32 = 60;
pub const DT: f32 = 1.0 / ACTION_TICKS_PER_SECOND as f32;
pub const BLOCK_WIDTH: usize = 36;
pub const BLOCK_HEIGHT: usize = 32;
pub const NUM_FLOORS: usize = 2; // Birch Street
pub const COUNTY_FLOORS: usize = 3; // Procedural County (0=1F, 1=2F, 2=3F)
pub const PLAYER_MAX_HEALTH: u8 = 6;

pub type Floor = u8; // 0 = ground (1F), 1 = 2F, 2 = 3F
pub type BuildingId = u16;
pub type CacheId = u16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ActionMode {
    BirchStreet = 0,
    #[default]
    County = 1,
}

/// Deterministic SplitMix64 hash for seeded variation and AI patrol rolls.
#[inline]
pub const fn mix64(mut n: u64) -> u64 {
    n = n.wrapping_add(0x9e37_79b9_7f4a_7c15);
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    n ^ (n >> 31)
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct Tile3D {
    pub x: i32,
    pub y: i32,
    pub floor: Floor,
}

impl Tile3D {
    pub const fn new(x: i32, y: i32, floor: Floor) -> Self {
        Self { x, y, floor }
    }

    pub const fn from_pos(pos: Position, floor: Floor) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            floor,
        }
    }

    pub fn from_vec2(pos: Vec2, floor: Floor) -> Self {
        Self::new(pos.x.round() as i32, pos.y.round() as i32, floor)
    }

    pub const fn pos(self) -> Position {
        Position::new(self.x, self.y)
    }

    pub fn center(self) -> Vec2 {
        Vec2::new(self.x as f32, self.y as f32)
    }

    pub fn manhattan_2d(self, other: Self) -> u32 {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec2,
    pub facing: Vec2,
    pub floor: Floor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Surface {
    Air,
    Grass,
    Forest,
    Field,
    Trail,
    Road,
    Sidewalk,
    Driveway,
    Gravel,
    Bridge,
    Water,
    Mountain,
    Porch,
    WoodFloor,
    KitchenTile,
    Carpet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterialCategory {
    None,
    Glass,
    Wood,
    Stone,
    Metal,
    Flesh,
    Caustic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Material {
    None,
    Glass,
    WoodLight,
    WoodDoor,
    Drywall,
    WoodSiding,
    BrickMasonry,
    MetalPole,
    TreeWood,
    Flesh,
    Caustic,
}

impl Material {
    pub const fn category(self) -> MaterialCategory {
        match self {
            Self::None => MaterialCategory::None,
            Self::Glass => MaterialCategory::Glass,
            Self::WoodLight | Self::WoodDoor | Self::TreeWood => MaterialCategory::Wood,
            Self::Drywall | Self::WoodSiding | Self::BrickMasonry => MaterialCategory::Stone,
            Self::MetalPole => MaterialCategory::Metal,
            Self::Flesh => MaterialCategory::Flesh,
            Self::Caustic => MaterialCategory::Caustic,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropKind {
    Table,
    Sofa,
    Counter,
    Bed,
    Crate,
    Fence,
    Mailbox,
    Bench,
    Shed,
    BirchTree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TileKind {
    Empty,
    ExteriorWall,
    InteriorWall,
    Pillar,
    Window,
    BrokenWindow,
    Door { open: bool, exterior: bool },
    StairsUp,
    StairsDown,
    StairsBoth,
    Streetlight,
    BrokenStreetlight,
    Prop(PropKind),
    Rubble,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildingStyle {
    BirchSage,
    BlueClapboard,
    ColonialBrick,
    WarmPutty,
    DowntownStorefront,
    DowntownOffice,
    DowntownApartments,
    HospitalClinical,
    PolicePrecinct,
    ArmyBarracks,
    WoodlandCabin,
    RuralBarn,
}

impl BuildingStyle {
    pub const fn has_pitched_roof(self) -> bool {
        matches!(
            self,
            Self::BirchSage
                | Self::BlueClapboard
                | Self::ColonialBrick
                | Self::WarmPutty
                | Self::WoodlandCabin
                | Self::RuralBarn
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingInfo {
    pub id: BuildingId,
    pub name: &'static str,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub floors: u8,
    pub style: BuildingStyle,
    pub front_door: Position,
    pub foundation_z: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TileVisibility {
    Hidden,
    Explored,
    Visible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WallOrientation {
    AlongX,
    AlongY,
    Corner,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TileRenderInfo {
    pub surface: Surface,
    pub kind: TileKind,
    pub material: Material,
    pub hp: u16,
    pub max_hp: u16,
    pub building_id: BuildingId,
    pub indoor: bool,
    pub light: u8,
    pub visibility: TileVisibility,
    pub orientation: WallOrientation,
    pub stair_dir: (i8, i8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LightKind {
    Streetlight,
    Porch,
    Cache,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSourceView {
    pub pos: Vec2,
    pub floor: Floor,
    pub radius: f32,
    pub intensity: u8,
    pub active: bool,
    pub kind: LightKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TileObstruction {
    Open,
    BrokenWindow,
    IntactWindow,
    DoorClosed,
    LowProp(Material),
    Wall(Material),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DamageKind {
    Axe,
    Bullet,
    ShotgunPellet,
    RocketBlast,
    RocketSplash,
    MonsterMelee,
    CausticBurst,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileDamageResult {
    pub damaged: bool,
    pub destroyed: bool,
    pub material: Material,
    pub old_kind: TileKind,
    pub new_kind: TileKind,
    pub sound_radius: f32,
    pub light_changed: bool,
    pub collapsed_cells: u8,
}

/// Environment interface shared between `block.rs`, `weapons.rs`, and `ai.rs`.
pub trait CombatWorld {
    /// Inclusive world XY bounds and number of floors.
    fn bounds(&self) -> (Position, Position, u8);
    /// Monotonic counter incremented whenever doors, windows, props, walls, or lights change.
    fn terrain_version(&self) -> u64;
    fn obstruction(&self, tile: Tile3D) -> TileObstruction;
    /// True if solid flooring or ground exists at `tile`.
    fn has_floor(&self, tile: Tile3D) -> bool;
    /// True if an actor can walk onto `tile`.
    fn is_walkable(&self, tile: Tile3D, can_vault_window: bool) -> bool;
    /// Building ID (`0` = outdoors, `1..` = buildings).
    fn building_id(&self, tile: Tile3D) -> BuildingId;
    /// If `tile` is a stair tile, returns `(paired_tile_on_other_floor, landing_dir)`.
    fn stair_link(&self, tile: Tile3D) -> Option<(Tile3D, (i8, i8))>;
    /// If `tile` is a stair tile (including 3-floor `StairsBoth`), returns up to 2 links.
    #[allow(clippy::type_complexity)]
    fn stair_links(&self, tile: Tile3D) -> ([Option<(Tile3D, (i8, i8))>; 2], usize) {
        match self.stair_link(tile) {
            Some(link) => ([Some(link), None], 1),
            None => ([None, None], 0),
        }
    }
    /// If `tile` is a broken F1 window or collapsed F1 exterior edge, returns F0 landing tile.
    fn window_drop_target(&self, tile: Tile3D) -> Option<Tile3D>;
    /// If `window` is a broken F0 window, returns the clear tile on the opposite side from `from`.
    fn window_vault_target(&self, from: Tile3D, window: Tile3D) -> Option<Tile3D>;
    /// Effective normalized light level at `tile` in `0.0..=1.0` (used for AI sight range).
    fn light_level(&self, tile: Tile3D) -> f32;
    /// Apply structural/prop damage to `tile`.
    fn damage_tile(&mut self, tile: Tile3D, kind: DamageKind) -> Option<TileDamageResult>;
    /// Open or close a door at `tile`; returns `Some(now_open)`.
    fn toggle_door(&mut self, tile: Tile3D) -> Option<bool>;
    /// Add a short-lived flash light (muzzle flash, rocket blast) without dirtying static light.
    fn register_dynamic_flash(
        &mut self,
        pos: Vec2,
        floor: Floor,
        radius: f32,
        intensity: u8,
        ttl_ticks: u8,
    );
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Weapon {
    Axe = 0,
    Pistol = 1,
    Shotgun = 2,
    RocketLauncher = 3,
}

impl Weapon {
    pub const COUNT: usize = 4;
    pub const ALL: [Self; Self::COUNT] =
        [Self::Axe, Self::Pistol, Self::Shotgun, Self::RocketLauncher];

    pub const fn slot(self) -> usize {
        self as usize
    }

    pub const fn bit(self) -> u8 {
        1u8 << (self as u8)
    }

    pub const fn from_slot(slot: u8) -> Option<Self> {
        match slot {
            0 => Some(Self::Axe),
            1 => Some(Self::Pistol),
            2 => Some(Self::Shotgun),
            3 => Some(Self::RocketLauncher),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheKind {
    Weapon,
    Ammo,
    Medkit,
    Mixed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheLoot {
    pub weapon: Option<Weapon>,
    pub pistol_ammo: u16,
    pub shotgun_ammo: u16,
    pub rockets: u16,
    pub medkits: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheView {
    pub id: CacheId,
    pub pos: Tile3D,
    pub kind: CacheKind,
    pub loot: CacheLoot,
    pub opened: bool,
    pub nearby: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum InteractPrompt {
    #[default]
    None = 0,
    Search = 1,
    OpenDoor = 2,
    CloseDoor = 3,
    ClimbWindow = 4,
    DropWindow = 5,
    StairsUp = 6,
    StairsDown = 7,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ObjectiveKind {
    #[default]
    Scavenge = 0,
    ClearBlock = 1,
    Won = 2,
    Dead = 3,
    Explore = 4,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProjectileId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectileKind {
    Rocket,
    ShooterBolt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectileView {
    pub id: ProjectileId,
    pub kind: ProjectileKind,
    pub pos: Vec2,
    pub dir: Vec2,
    pub floor: Floor,
    pub z: f32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct MonsterId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MonsterKind {
    Shambler = 0,
    Brisk = 1,
    Bloater = 2,
    Shooter = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AwarenessIcon {
    Unaware = 0,
    Investigating = 1,
    Alert = 2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonsterView {
    pub id: MonsterId,
    pub kind: MonsterKind,
    pub variant: u8,
    pub pos: Vec2,
    pub facing: Vec2,
    pub floor: Floor,
    pub elevation: f32,
    pub hp: u8,
    pub max_hp: u8,
    pub awareness: AwarenessIcon,
    pub telegraph_progress: Option<f32>,
    pub telegraph_dir: Vec2,
    pub telegraph_target: Option<(Vec2, Floor)>,
    pub hitstun: bool,
    pub swelling: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoiseKind {
    Footstep,
    WindowVault,
    WindowDrop,
    DoorToggle,
    AxeSwing,
    GlassShatter,
    PropBreak,
    PistolShot,
    ShotgunBlast,
    RocketLaunch,
    RocketBlast,
    BloaterBurst,
    ShooterShot,
    MedkitUse,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseEvent {
    pub origin: Vec2,
    pub floor: Floor,
    pub radius: f32,
    pub priority: u8,
    pub kind: NoiseKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActionInput {
    /// Directional movement, with analog magnitude preserved up to one (`<= 0.52` is quiet walk).
    pub movement: Vec2,
    /// Aim direction. A zero vector or `None` preserves the prior aim direction.
    pub aim: Option<Vec2>,
    pub fire: bool,
    pub interact: bool,
    pub select_weapon: Option<Weapon>,
    pub cycle_weapon: bool,
    pub reload: bool,
    pub use_medkit: bool,
    pub quiet_walk: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionOutcome {
    Playing,
    Dead,
    Won,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloorTransition {
    Stairs,
    WindowDrop,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActionEvent {
    // Weapons & projectiles
    UnarmedShove {
        from: Vec2,
        floor: Floor,
        facing: Vec2,
    },
    AxeSwing {
        from: Vec2,
        floor: Floor,
        facing: Vec2,
        hit_flesh: bool,
        hit_solid: bool,
    },
    ShotFired {
        weapon: Weapon,
        from: Vec2,
        floor: Floor,
        facing: Vec2,
    },
    BulletTrace {
        weapon: Weapon,
        from: Vec2,
        from_z: f32,
        to: Vec2,
        to_z: f32,
        hit_material: MaterialCategory,
    },
    ProjectileLaunched {
        id: ProjectileId,
        kind: ProjectileKind,
        from: Vec2,
        floor: Floor,
        dir: Vec2,
    },
    RocketBlast {
        at: Vec2,
        floor: Floor,
        radius: f32,
    },
    DryFire {
        weapon: Weapon,
    },

    // Environment & destruction
    MaterialHit {
        at: Tile3D,
        hit_pos: Vec2,
        material: Material,
        destroyed: bool,
        light_changed: bool,
    },
    StructuralCollapse {
        at: Tile3D,
    },
    DoorToggled {
        at: Tile3D,
        opened: bool,
    },
    WindowClimbed {
        at: Tile3D,
    },

    // Inventory, reload & healing
    WeaponSwitched {
        weapon: Option<Weapon>,
    },
    ReloadStarted {
        weapon: Weapon,
        duration_ticks: u16,
    },
    ReloadCompleted {
        weapon: Weapon,
        loaded: u8,
    },
    Looted {
        at: Tile3D,
        loot: CacheLoot,
    },
    Healed {
        at: Vec2,
        floor: Floor,
        amount: u8,
        new_health: u8,
    },

    // Monster AI & combat
    MonsterAlerted {
        id: MonsterId,
        kind: MonsterKind,
        at: Vec2,
        floor: Floor,
        by_sight: bool,
    },
    MonsterTelegraph {
        id: MonsterId,
        kind: MonsterKind,
        at: Vec2,
        floor: Floor,
        dir: Vec2,
        duration_ticks: u16,
    },
    MonsterHit {
        id: MonsterId,
        kind: MonsterKind,
        at: Vec2,
        floor: Floor,
        damage: u8,
        remaining_hp: u8,
        knockback_dir: Vec2,
    },
    MonsterDown {
        id: MonsterId,
        kind: MonsterKind,
        at: Vec2,
        floor: Floor,
        overkill: bool,
    },
    BloaterSwell {
        id: MonsterId,
        at: Vec2,
        floor: Floor,
        fuse_ticks: u16,
    },
    BloaterBurst {
        id: MonsterId,
        at: Vec2,
        floor: Floor,
        radius: f32,
    },

    // Player & run state
    PlayerHurt {
        at: Vec2,
        floor: Floor,
        damage: u8,
        remaining_hp: u8,
        from: Vec2,
    },
    FloorChanged {
        from_floor: Floor,
        to_floor: Floor,
        at: Tile3D,
        via: FloorTransition,
    },
    Died,
    Won,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionHud {
    pub tick: u64,
    pub health: u8,
    pub max_health: u8,
    pub floor: Floor,
    /// Convenience total ammo for equipped firearm (kept for v1 FFI compatibility).
    pub ammo: u16,
    /// True once player holds any weapon.
    pub armed: bool,
    pub equipped: Option<Weapon>,
    pub owned_weapons_mask: u8,
    pub active_mag: u8,
    pub active_mag_cap: u8,
    pub active_reserve: u16,
    pub pistol_mag: u8,
    pub pistol_reserve: u16,
    pub shotgun_mag: u8,
    pub shotgun_reserve: u16,
    pub rocket_mag: u8,
    pub rocket_reserve: u16,
    pub reloading: bool,
    pub can_reload: bool,
    pub reload_progress: u8, // 0..=100
    pub medkits: u8,
    pub can_heal: bool,
    pub zombie_count: usize,
    pub zombies_total: usize,
    pub caches_found: u8,
    pub caches_total: u8,
    pub interact_prompt: InteractPrompt,
    pub objective_kind: ObjectiveKind,
    pub rocket_danger_close: bool,
    pub indoors: bool,
    pub outcome: ActionOutcome,
    pub player_pos: Position,
    pub district: District,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerRenderState {
    pub pose: Pose,
    pub elevation: f32,
    pub equipped: Option<Weapon>,
    pub ammo_in_equipped: u16,
    pub health: u8,
    pub max_health: u8,
    pub indoors: bool,
    pub quiet_walking: bool,
    pub near_interact_tile: Option<Tile3D>,
    pub interact_prompt: InteractPrompt,
    pub rocket_danger_close: bool,
}
