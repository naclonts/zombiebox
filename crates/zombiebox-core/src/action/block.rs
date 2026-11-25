//! Destructible multi-level Birch Street residential block, lighting, visibility, and physics.

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::{CHUNK_SIZE, House, Position, World};

use super::types::{
    BLOCK_HEIGHT, BLOCK_WIDTH, BuildingInfo, BuildingStyle, CacheKind, CacheLoot, CacheView,
    CombatWorld, DamageKind, Floor, LightKind, LightSourceView, Material, MonsterKind, NUM_FLOORS,
    PropKind, Surface, Tile3D, TileDamageResult, TileKind, TileObstruction, TileRenderInfo,
    TileVisibility, Vec2, WallOrientation, Weapon, mix64,
};

const OUTDOOR_AMBIENT: f32 = 92.0;
const INDOOR_AMBIENT: f32 = 42.0;
const STREETLIGHT_RADIUS: f32 = 6.5;
const STREETLIGHT_INTENSITY: u8 = 155;
const STREETLIGHT_F1_BLEED: f32 = 0.55;
const PORCH_LIGHT_RADIUS: f32 = 3.5;
const PORCH_LIGHT_INTENSITY: u8 = 125;
const WINDOW_LIGHT_TRANSMIT: f32 = 0.85;
const TREE_LIGHT_TRANSMIT: f32 = 0.65;
const VISIBILITY_RANGE: f32 = 14.0;
const MAX_DYNAMIC_FLASHES: usize = 32;

const EXTERIOR_MAP: [&str; BLOCK_HEIGHT] = [
    "sRs...........T.sRs..T.......T..sRss", // 00
    "sRsT............sRsT...T......T.sRss", // 01
    "sRs.c...........sRs..b..........sRss", // 02
    "sRs.............sRs..b...g......sRss", // 03
    "sRsddd..........sRs.....gSSSc...sRss", // 04
    "sRsddd..........sRsgggggggpSggggsRss", // 05
    "sRsddd..........sRs.....ggg.....sRss", // 06
    "sRsddd..........sRs..b...g...b..sRss", // 07
    "sRsddd..........sRs.T....g....T.sRss", // 08
    "sRsddd..........sRs......g......sRss", // 09
    "sRssssLssssMsssssRsssssssssssssssRss", // 10
    "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR", // 11
    "sRssssssssssssssLRsssssssssssssssRss", // 12
    "sRs.fff..fffff..sRs.fffff..fff.TsRss", // 13
    "sRsf...........csRs...........c.sRss", // 14
    "sRsf...........TsRs.............sRss", // 15
    "sRsf............sRs.............sRss", // 16
    "sRsf............sRs...........ddsRss", // 17
    "sRsf..........ddsRs...........ddsRss", // 18
    "sRsf..........ddsRs...........ddsRss", // 19
    "sRs...........ddsRs...........ddsRss", // 20
    "sRs...........ddsRs...........ddsRss", // 21
    "sRs...........ddsRs...........ddsRss", // 22
    "sRs...........ddsRs...........ddsRss", // 23
    "sRsT....ppp...ddsRs...p....p..ddsRss", // 24
    "sRs.....ms....ddsRs..ms....sm.ddsRss", // 25
    "sRssLsssssssssssLRsssssssssssssLsRss", // 26
    "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR", // 27
    "sRsssssssssssssssRLssssssssssssssRss", // 28
    "sRs.......b.....sRs.............sRss", // 29
    "sRs..T........T.sRs...T.......T.sRss", // 30
    "sRs..fff....fff.sRs..ffff..ffff.sRss", // 31
];

struct HouseBlueprint {
    id: u8,
    name: &'static str,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    style: BuildingStyle,
    wall_mat: Material,
    corner_mat: Material,
    front_door: (usize, usize),
    f0: &'static [&'static str],
    f1: &'static [&'static str],
}

// House blueprint legend:
//   'P' = Corner Pillar (corner_mat, load_bearing on F0)
//   '#' = Exterior Wall (wall_mat, load_bearing on F0)
//   'W' = Exterior Window (Glass)
//   'O' = Open Exterior Door (WoodDoor)
//   'D' = Closed Exterior Door (WoodDoor)
//   'i' = Interior Wall (Drywall)
//   'd' = Closed Interior Door (WoodDoor)
//   'U' = StairsUp (WoodFloor, landing West (-1, 0))
//   'V' = StairsDown (WoodFloor, landing West (-1, 0))
//   '.' = WoodFloor (Empty)
//   'k' = KitchenTile (Empty)
//   ',' = Carpet (Empty)
//   'c' = Counter on KitchenTile
//   't' = Table on WoodFloor
//   'q' = Table on KitchenTile
//   's' = Sofa on WoodFloor
//   'b' = Bed on Carpet
const HOUSE_BLUEPRINTS: [HouseBlueprint; 3] = [
    // House 1 — Birch Bungalow: 9x8 at local (5, 16)
    // Front open door (9, 23), stairs (12, 21), Cache 1 (7, 20, F0), Cache 2 (11, 17, F1)
    HouseBlueprint {
        id: 1,
        name: "Birch Bungalow",
        x: 5,
        y: 16,
        width: 9,
        height: 8,
        style: BuildingStyle::BirchSage,
        wall_mat: Material::WoodSiding,
        corner_mat: Material::WoodSiding,
        front_door: (9, 23),
        f0: &[
            "P#W###D#P", // y=16: N window (7,16), back door (11,16)
            "#ckqi...#", // y=17: Kitchen (6..=8,17..=18), counter (6,17), table (8,17)
            "Wkkkd...W", // y=18: W window (5,18), interior door (9,18), E window (13,18)
            "#iiii...#", // y=19: Interior divider wall
            "#s...t..#", // y=20: Living room: sofa (6,20), Cache 1 at (7,20), table (10,20)
            "W......U#", // y=21: W window (5,21), StairsUp (12,21), clear west landing (11,21)
            "#.......#", // y=22: Clear front entry
            "P#W#O#W#P", // y=23: Open front door (9,23), windows (7,23) & (11,23)
        ],
        f1: &[
            "P#W#####P", // y=16: N window (7,16)
            "#b,,it..#", // y=17: Master bedroom (6..=8), Study (10..=12,17..=18): table (10,17), Cache 2 (11,17)
            "#,,,i...W", // y=18: E window (13,18)
            "#,,,iidi#", // y=19: Study interior door (11,19)
            "W,,,i...#", // y=20: W window (5,20)
            "#,,,d..V#", // y=21: Master bedroom door (9,21), StairsDown (12,21), clear west landing (11,21)
            "#t,,i...#", // y=22: SW desk (6,22)
            "P#W###W#P", // y=23: S windows (7,23) & (11,23)
        ],
    },
    // House 2 — Elm Corner Duplex: 10x9 at local (20, 15)
    // Front doors (22, 23) & (27, 23), stairs (28, 17), Cache 3 (26, 17, F0), Cache 4 (22, 21, F1)
    HouseBlueprint {
        id: 2,
        name: "Elm Corner Duplex",
        x: 20,
        y: 15,
        width: 10,
        height: 9,
        style: BuildingStyle::ColonialBrick,
        wall_mat: Material::BrickMasonry,
        corner_mat: Material::BrickMasonry,
        front_door: (22, 23),
        f0: &[
            "P#W##W#D#P", // y=15: N windows (22,15),(25,15), back door (27,15)
            "#ckqi...c#", // y=16: W kitchenette (21..=23,16..=17), E unit north (25..=28,16..=19)
            "Wkkki...U#", // y=17: W window (20,17), Cache 3 (26,17), landing (27,17), StairsUp (28,17)
            "#idii....W", // y=18: Interior door (22,18), E window (29,18)
            "#...d....#", // y=19: Duplex connecting door (24,19)
            "W...iidii#", // y=20: W window (20,20), East divider with door (26,20)
            "#s..i....W", // y=21: W lounge sofa (21,21), E window (29,21)
            "#...i...s#", // y=22: Clear entries, E sofa (28,22)
            "P#D##W#D#P", // y=23: Front doors (22,23) & (27,23), window (25,23)
        ],
        f1: &[
            "P#W###W##P", // y=15: N windows (22,15),(26,15)
            "#b,,i....#", // y=16: NW bedroom (21..=23,16..=18), NE landing (25..=28,16..=19)
            "W,,,d...V#", // y=17: W window (20,17), door (24,17), landing (27,17), StairsDown (28,17)
            "#,,,i....W", // y=18: E window (29,18)
            "#idii....#", // y=19: Door (22,19) to SW study
            "W...iidii#", // y=20: W window (20,20), door (26,20) to SE bedroom
            "#...i,,,,W", // y=21: SW study Cache 4 (22,21), SE bedroom (25..=28,21..=22), E window (29,21)
            "#..ti,,,b#", // y=22: SW desk (23,22), SE bed (28,22)
            "P#W###W##P", // y=23: S windows (22,23),(26,23)
        ],
    },
    // House 3 — Oak Ridge Split-Level: 10x8 at local (6, 2)
    // Corner pillars BrickMasonry, walls WoodSiding, front door (10, 9), stairs (14, 5),
    // Cache 5 (8, 4, F1), Cache 6 (13, 4, F0)
    HouseBlueprint {
        id: 3,
        name: "Oak Ridge Split-Level",
        x: 6,
        y: 2,
        width: 10,
        height: 8,
        style: BuildingStyle::BlueClapboard,
        wall_mat: Material::WoodSiding,
        corner_mat: Material::BrickMasonry,
        front_door: (10, 9),
        f0: &[
            "P#W####W#P", // y=2: N windows (8,2),(13,2)
            "#s...ickc#", // y=3: Living sofa (7,3), Kitchen counters (12,3),(14,3)
            "#....dkkk#", // y=4: Interior door (11,4), Kitchen Cache 6 (13,4)
            "D....i..U#", // y=5: W ext door (6,5), StairsUp (14,5), landing (13,5)
            "#..t.idii#", // y=6: Table (9,6), door (12,6) to SE dining
            "W....d...W", // y=7: W window (6,7), door (11,7), E window (15,7)
            "#....i.t.#", // y=8: SE dining table (13,8)
            "P#W#D#W##P", // y=9: S windows (8,9),(12,9), front door (10,9)
        ],
        f1: &[
            "P#W####W#P", // y=2: N windows (8,2),(13,2)
            "#b,,,i...#", // y=3: Master Suite NW (7..=10,3..=5): bed (7,3); NE hall (12..=14,3..=5)
            "#,,,,d...#", // y=4: Cache 5 Rocket Launcher (8,4), door (11,4)
            "W,,,,i..V#", // y=5: W window (6,5), StairsDown (14,5), landing (13,5)
            "#idiiidii#", // y=6: Doors (8,6) & (12,6) to south bedrooms
            "W,,,,i,,,W", // y=7: W window (6,7), E window (15,7)
            "#,,,bi,,b#", // y=8: SW bed (10,8), SE bed (14,8)
            "P#W###W##P", // y=9: S windows (8,9),(12,9)
        ],
    },
];

// Streetlights (6) at exact local coordinates specified for the residential block.
const STREETLIGHT_POSITIONS: [(usize, usize); 6] =
    [(16, 26), (18, 28), (4, 26), (31, 26), (16, 12), (6, 10)];

// Porch lights (5): House 1 front, House 2 west & east fronts, House 3 front, NE Park shed.
const PORCH_LIGHT_POSITIONS: [(usize, usize); 5] = [(9, 23), (22, 23), (27, 23), (10, 9), (26, 5)];

/// Material specification `(Material, max_hp)` for any tile kind.
pub fn tile_spec(kind: TileKind, wall_mat: Material) -> (Material, u16) {
    match kind {
        TileKind::Empty
        | TileKind::BrokenWindow
        | TileKind::BrokenStreetlight
        | TileKind::Rubble
        | TileKind::StairsUp
        | TileKind::StairsDown => (Material::None, 0),
        TileKind::Window => (Material::Glass, 20),
        TileKind::Door { .. } => (Material::WoodDoor, 60),
        TileKind::Streetlight => (Material::MetalPole, 50),
        TileKind::InteriorWall => (Material::Drywall, 1000),
        TileKind::ExteriorWall | TileKind::Pillar => {
            let mat = match wall_mat {
                Material::None => Material::WoodSiding,
                other => other,
            };
            (mat, 1000)
        }
        TileKind::Prop(prop) => match prop {
            PropKind::Mailbox => (Material::WoodLight, 25),
            PropKind::Fence => (Material::WoodLight, 40),
            PropKind::Crate | PropKind::Table | PropKind::Bench => (Material::WoodLight, 50),
            PropKind::Sofa | PropKind::Bed | PropKind::Counter => (Material::WoodLight, 75),
            PropKind::Shed => (Material::WoodLight, 80),
            PropKind::BirchTree => (Material::TreeWood, 120),
        },
    }
}

/// Damage dealt by `kind` against a structure or prop of material `mat`.
pub fn damage_against(mat: Material, kind: DamageKind) -> u16 {
    use DamageKind::{
        Axe, Bullet, CausticBurst, MonsterMelee, RocketBlast, RocketSplash, ShotgunPellet,
    };
    match mat {
        Material::Glass => match kind {
            Axe | Bullet | CausticBurst => 25,
            ShotgunPellet | MonsterMelee => 15,
            RocketBlast => 200,
            RocketSplash => 100,
        },
        Material::WoodLight => match kind {
            Axe | CausticBurst => 25,
            Bullet => 15,
            ShotgunPellet => 10,
            RocketBlast => 300,
            RocketSplash => 120,
            MonsterMelee => 12,
        },
        Material::WoodDoor => match kind {
            Axe => 20,
            Bullet => 15,
            ShotgunPellet | MonsterMelee => 12,
            RocketBlast => 300,
            RocketSplash => 120,
            CausticBurst => 25,
        },
        Material::TreeWood => match kind {
            Axe | CausticBurst => 20,
            Bullet => 10,
            ShotgunPellet | MonsterMelee => 8,
            RocketBlast => 300,
            RocketSplash => 120,
        },
        Material::MetalPole => match kind {
            Axe | Bullet => 25,
            ShotgunPellet => 12,
            RocketBlast => 300,
            RocketSplash => 120,
            MonsterMelee => 5,
            CausticBurst => 20,
        },
        Material::Drywall => match kind {
            Axe | ShotgunPellet | MonsterMelee => 2,
            Bullet => 4,
            RocketBlast => 1200,
            RocketSplash => 350,
            CausticBurst => 15,
        },
        Material::WoodSiding | Material::BrickMasonry => match kind {
            Axe | ShotgunPellet | MonsterMelee => 1,
            Bullet => 2,
            RocketBlast => 1200,
            RocketSplash => 380,
            CausticBurst => 10,
        },
        Material::None | Material::Flesh | Material::Caustic => 0,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupplyCache {
    pub id: u8,
    pub pos: Tile3D,
    pub kind: CacheKind,
    pub loot: CacheLoot,
    pub opened: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonsterSpawn {
    pub pos: Tile3D,
    pub kind: MonsterKind,
    pub variant: u8,
}

#[derive(Clone, Copy, Debug)]
struct Cell {
    surface: Surface,
    kind: TileKind,
    material: Material,
    hp: u16,
    max_hp: u16,
    building_id: u8,
    indoor: bool,
    load_bearing: bool,
    collapsed: bool,
    room_id: u16,
    visibility: TileVisibility,
    stair_dir: (i8, i8),
}

impl Cell {
    const fn air() -> Self {
        Self {
            surface: Surface::Air,
            kind: TileKind::Empty,
            material: Material::None,
            hp: 0,
            max_hp: 0,
            building_id: 0,
            indoor: false,
            load_bearing: false,
            collapsed: false,
            room_id: 0,
            visibility: TileVisibility::Hidden,
            stair_dir: (0, 0),
        }
    }

    fn set_kind(&mut self, kind: TileKind, wall_mat: Material) {
        let (material, max_hp) = tile_spec(kind, wall_mat);
        self.kind = kind;
        self.material = material;
        self.hp = max_hp;
        self.max_hp = max_hp;
    }
}

#[derive(Clone, Copy, Debug)]
struct StaticLight {
    lx: usize,
    ly: usize,
    floor: Floor,
    radius: f32,
    intensity: u8,
    active: bool,
    kind: LightKind,
}

#[derive(Clone, Copy, Debug)]
struct DynamicFlash {
    pos: Vec2,
    floor: Floor,
    radius: f32,
    intensity: u8,
    ttl_ticks: u8,
}

#[derive(Clone, Debug)]
struct StaticLightCache {
    version: u64,
    levels: Vec<u8>,
}

pub struct ResidentialBlock {
    origin: Position,
    cells: Vec<Cell>,
    buildings: Vec<BuildingInfo>,
    caches: Vec<SupplyCache>,
    monster_spawns: Vec<MonsterSpawn>,
    static_lights: Vec<StaticLight>,
    flashes: Vec<DynamicFlash>,
    light_cache: RefCell<StaticLightCache>,
    terrain_version: u64,
    pending_collapses: Vec<Tile3D>,
}

impl ResidentialBlock {
    pub fn new(world: &World, seed: u64) -> Self {
        let wake = world.wake_position();
        let cx = wake.x.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
        let cy = wake.y.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
        let origin = Position::new(cx - 9, cy - 19);

        let total_cells = NUM_FLOORS * BLOCK_HEIGHT * BLOCK_WIDTH;
        let mut block = Self {
            origin,
            cells: vec![Cell::air(); total_cells],
            buildings: Vec::with_capacity(HOUSE_BLUEPRINTS.len()),
            caches: Vec::with_capacity(8),
            monster_spawns: Vec::with_capacity(12),
            static_lights: Vec::with_capacity(11),
            flashes: Vec::new(),
            light_cache: RefCell::new(StaticLightCache {
                version: u64::MAX,
                levels: vec![0; total_cells],
            }),
            terrain_version: 1,
            pending_collapses: Vec::new(),
        };

        block.stamp_exterior();
        block.stamp_houses();
        block.init_lights();
        block.init_caches();
        block.init_monster_spawns();
        block.apply_seeded_scatter(seed);
        block.recompute_rooms();
        block
    }

    pub fn origin(&self) -> Position {
        self.origin
    }

    pub fn player_spawn(&self) -> Tile3D {
        self.local_to_world(9, 25, 0)
    }

    pub fn starter_house(&self) -> House {
        let bp = &HOUSE_BLUEPRINTS[0];
        House {
            x: self.origin.x + bp.x as i32,
            y: self.origin.y + bp.y as i32,
            width: bp.width as i32,
            height: bp.height as i32,
            door: Position::new(
                self.origin.x + bp.front_door.0 as i32,
                self.origin.y + bp.front_door.1 as i32,
            ),
        }
    }

    pub fn buildings(&self) -> &[BuildingInfo] {
        &self.buildings
    }

    pub fn caches(&self) -> &[SupplyCache] {
        &self.caches
    }

    pub fn take_cache(&mut self, id: u8) -> Option<CacheLoot> {
        let cache = self.caches.iter_mut().find(|c| c.id == id)?;
        if cache.opened {
            return None;
        }
        cache.opened = true;
        Some(cache.loot)
    }

    pub fn cache_views(
        &self,
        player_pos: Vec2,
        player_floor: Floor,
        interact_radius: f32,
    ) -> Vec<CacheView> {
        self.caches
            .iter()
            .map(|c| CacheView {
                id: c.id,
                pos: c.pos,
                kind: c.kind,
                loot: c.loot,
                opened: c.opened,
                nearby: !c.opened
                    && c.pos.floor == player_floor
                    && c.pos.center().distance(player_pos) <= interact_radius,
            })
            .collect()
    }

    pub fn monster_spawns(&self) -> &[MonsterSpawn] {
        &self.monster_spawns
    }

    pub fn tile_render_info(&self, pos: Position, floor: Floor) -> Option<TileRenderInfo> {
        let tile = Tile3D::from_pos(pos, floor);
        let (lx, ly, fl) = self.world_to_local(tile)?;
        let cell = self.cell_at(lx, ly, fl);
        let light = self.effective_light_u8(tile);
        let orientation = self.wall_orientation(lx, ly, fl);
        Some(TileRenderInfo {
            surface: cell.surface,
            kind: cell.kind,
            material: cell.material,
            hp: cell.hp,
            max_hp: cell.max_hp,
            building_id: cell.building_id,
            indoor: cell.indoor,
            light,
            visibility: cell.visibility,
            orientation,
            stair_dir: cell.stair_dir,
        })
    }

    pub fn light_sources(&self) -> Vec<LightSourceView> {
        let mut views = Vec::with_capacity(self.static_lights.len() + self.caches.len());
        for light in &self.static_lights {
            let world_tile = self.local_to_world(light.lx, light.ly, light.floor);
            views.push(LightSourceView {
                pos: world_tile.center(),
                floor: light.floor,
                radius: light.radius,
                intensity: light.intensity,
                active: light.active,
                kind: light.kind,
            });
        }
        for cache in &self.caches {
            views.push(LightSourceView {
                pos: cache.pos.center(),
                floor: cache.pos.floor,
                radius: 1.8,
                intensity: 48,
                active: !cache.opened,
                kind: LightKind::Cache,
            });
        }
        views
    }

    pub fn effective_light_u8(&self, tile: Tile3D) -> u8 {
        let Some((lx, ly, fl)) = self.world_to_local(tile) else {
            return OUTDOOR_AMBIENT as u8;
        };
        self.ensure_static_light();
        let base = self.light_cache.borrow().levels[Self::idx(lx, ly, fl)] as u16;
        if self.flashes.is_empty() {
            return base as u8;
        }
        let center = tile.center();
        let mut extra = 0.0_f32;
        for flash in &self.flashes {
            if flash.ttl_ticks == 0 || flash.radius <= 0.0 {
                continue;
            }
            let floor_scale = if flash.floor == tile.floor { 1.0 } else { 0.25 };
            let dist = center.distance(flash.pos);
            if dist < flash.radius {
                let att = (1.0 - dist / flash.radius) * floor_scale;
                extra += flash.intensity as f32 * att;
            }
        }
        (base + extra.round() as u16).min(255) as u8
    }

    pub fn is_indoor(&self, tile: Tile3D) -> bool {
        self.world_to_local(tile)
            .is_some_and(|(lx, ly, fl)| self.cell_at(lx, ly, fl).indoor)
    }

    pub fn tile_kind(&self, tile: Tile3D) -> TileKind {
        self.world_to_local(tile)
            .map_or(TileKind::ExteriorWall, |(lx, ly, fl)| {
                self.cell_at(lx, ly, fl).kind
            })
    }

    pub fn tick_flashes(&mut self) {
        for flash in &mut self.flashes {
            flash.ttl_ticks = flash.ttl_ticks.saturating_sub(1);
        }
        self.flashes.retain(|flash| flash.ttl_ticks > 0);
    }

    pub fn update_visibility(&mut self, viewer: Vec2, floor: Floor) {
        if floor as usize >= NUM_FLOORS {
            return;
        }
        for cell in &mut self.cells {
            if cell.visibility == TileVisibility::Visible {
                cell.visibility = TileVisibility::Explored;
            }
        }

        let viewer_local = Vec2::new(
            viewer.x - self.origin.x as f32,
            viewer.y - self.origin.y as f32,
        );
        let v_lx = viewer_local.x.round() as i32;
        let v_ly = viewer_local.y.round() as i32;
        let fl = floor as usize;

        let mut revealed_rooms: Vec<u16> = Vec::new();
        let mut mark_visible = |cells: &mut [Cell], lx: usize, ly: usize, f: usize| {
            let cell = &mut cells[Self::idx(lx, ly, f)];
            cell.visibility = TileVisibility::Visible;
            if cell.room_id != 0 && !revealed_rooms.contains(&cell.room_id) {
                revealed_rooms.push(cell.room_id);
            }
        };

        if (0..BLOCK_WIDTH as i32).contains(&v_lx) && (0..BLOCK_HEIGHT as i32).contains(&v_ly) {
            mark_visible(&mut self.cells, v_lx as usize, v_ly as usize, fl);
        }

        let r_ceil = VISIBILITY_RANGE.ceil() as i32;
        let min_x = (v_lx - r_ceil).max(0) as usize;
        let max_x = (v_lx + r_ceil).min(BLOCK_WIDTH as i32 - 1) as usize;
        let min_y = (v_ly - r_ceil).max(0) as usize;
        let max_y = (v_ly + r_ceil).min(BLOCK_HEIGHT as i32 - 1) as usize;
        let range_sq = VISIBILITY_RANGE * VISIBILITY_RANGE;

        for ly in min_y..=max_y {
            for lx in min_x..=max_x {
                let dx = lx as f32 - viewer_local.x;
                let dy = ly as f32 - viewer_local.y;
                if dx * dx + dy * dy > range_sq {
                    continue;
                }
                if self.los_reaches(viewer_local, lx, ly, fl) {
                    mark_visible(&mut self.cells, lx, ly, fl);
                }
            }
        }

        // Full room reveal when standing in or peeking into any room via window/door/breach.
        if !revealed_rooms.is_empty() {
            let mut room_tiles: Vec<(usize, usize, u8)> = Vec::new();
            for ly in 0..BLOCK_HEIGHT {
                for lx in 0..BLOCK_WIDTH {
                    let cell = &mut self.cells[Self::idx(lx, ly, fl)];
                    if cell.room_id != 0 && revealed_rooms.contains(&cell.room_id) {
                        cell.visibility = TileVisibility::Visible;
                        room_tiles.push((lx, ly, cell.building_id));
                    }
                }
            }
            for (lx, ly, bid) in room_tiles {
                for dy in -1..=1_i32 {
                    for dx in -1..=1_i32 {
                        let nx = lx as i32 + dx;
                        let ny = ly as i32 + dy;
                        if !(0..BLOCK_WIDTH as i32).contains(&nx)
                            || !(0..BLOCK_HEIGHT as i32).contains(&ny)
                        {
                            continue;
                        }
                        let ncell = &mut self.cells[Self::idx(nx as usize, ny as usize, fl)];
                        if ncell.building_id == bid && is_room_barrier(ncell.kind) {
                            ncell.visibility = TileVisibility::Visible;
                        }
                    }
                }
            }
        }

        // F1-to-F0 exterior sighting through F1 windows, breached walls, and collapsed edges.
        if floor == 1 {
            for ly in 0..BLOCK_HEIGHT {
                for lx in 0..BLOCK_WIDTH {
                    let f1 = self.cells[Self::idx(lx, ly, 1)];
                    if f1.visibility == TileVisibility::Visible && f1.surface == Surface::Air {
                        let f0 = &mut self.cells[Self::idx(lx, ly, 0)];
                        if !f0.indoor {
                            f0.visibility = TileVisibility::Visible;
                        }
                    }
                }
            }
        }
    }

    pub fn take_collapsed_tiles(&mut self) -> Vec<Tile3D> {
        std::mem::take(&mut self.pending_collapses)
    }

    pub fn circle_walkable(
        &self,
        center: Vec2,
        floor: Floor,
        radius: f32,
        can_vault_window: bool,
    ) -> bool {
        let min_x = (center.x - radius - 0.5).floor() as i32;
        let max_x = (center.x + radius + 0.5).ceil() as i32;
        let min_y = (center.y - radius - 0.5).floor() as i32;
        let max_y = (center.y + radius + 0.5).ceil() as i32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let tile = Tile3D::new(x, y, floor);
                if self.is_walkable(tile, can_vault_window) {
                    continue;
                }
                let nearest_x = center.x.clamp(x as f32 - 0.5, x as f32 + 0.5);
                let nearest_y = center.y.clamp(y as f32 - 0.5, y as f32 + 0.5);
                let dx = center.x - nearest_x;
                let dy = center.y - nearest_y;
                if dx * dx + dy * dy < radius * radius {
                    return false;
                }
            }
        }
        true
    }

    pub fn move_body(
        &self,
        position: &mut Vec2,
        floor: Floor,
        delta: Vec2,
        radius: f32,
        can_vault_window: bool,
        doorway_assist: bool,
    ) {
        let steps = ((delta.length() / 0.08).ceil() as usize).max(1);
        let part = delta * (1.0 / steps as f32);

        for _ in 0..steps {
            let cand_x = Vec2::new(position.x + part.x, position.y);
            if self.circle_walkable(cand_x, floor, radius, can_vault_window) {
                position.x = cand_x.x;
            } else if doorway_assist && part.x.abs() > 1e-4 {
                let ahead_x = (position.x + part.x.signum() * (radius + 0.18)).round() as i32;
                if let Some(target_y) =
                    self.find_doorway_align(ahead_x, position.y, floor, can_vault_window, true)
                {
                    let nudge =
                        (target_y - position.y).clamp(-part.x.abs() * 0.85, part.x.abs() * 0.85);
                    let nudged = Vec2::new(position.x, position.y + nudge);
                    if self.circle_walkable(nudged, floor, radius, can_vault_window) {
                        position.y = nudged.y;
                        let retry = Vec2::new(position.x + part.x, position.y);
                        if self.circle_walkable(retry, floor, radius, can_vault_window) {
                            position.x = retry.x;
                        }
                    }
                }
            }

            let cand_y = Vec2::new(position.x, position.y + part.y);
            if self.circle_walkable(cand_y, floor, radius, can_vault_window) {
                position.y = cand_y.y;
            } else if doorway_assist && part.y.abs() > 1e-4 {
                let ahead_y = (position.y + part.y.signum() * (radius + 0.18)).round() as i32;
                if let Some(target_x) =
                    self.find_doorway_align(ahead_y, position.x, floor, can_vault_window, false)
                {
                    let nudge =
                        (target_x - position.x).clamp(-part.y.abs() * 0.85, part.y.abs() * 0.85);
                    let nudged = Vec2::new(position.x + nudge, position.y);
                    if self.circle_walkable(nudged, floor, radius, can_vault_window) {
                        position.x = nudged.x;
                        let retry = Vec2::new(position.x, position.y + part.y);
                        if self.circle_walkable(retry, floor, radius, can_vault_window) {
                            position.y = retry.y;
                        }
                    }
                }
            }
        }
    }

    fn find_doorway_align(
        &self,
        ahead_axis: i32,
        perp_coord: f32,
        floor: Floor,
        can_vault_window: bool,
        ahead_is_x: bool,
    ) -> Option<f32> {
        let base = perp_coord.round() as i32;
        let mut best: Option<(f32, f32)> = None;
        for offset in [-1, 0, 1] {
            let cand = base + offset;
            let dist = (cand as f32 - perp_coord).abs();
            if dist > 0.42 || dist < 0.01 {
                continue;
            }
            let tile = if ahead_is_x {
                Tile3D::new(ahead_axis, cand, floor)
            } else {
                Tile3D::new(cand, ahead_axis, floor)
            };
            if self.is_walkable(tile, can_vault_window)
                && best.is_none_or(|(_, best_dist)| dist < best_dist)
            {
                best = Some((cand as f32, dist));
            }
        }
        best.map(|(coord, _)| coord)
    }

    #[inline]
    const fn idx(lx: usize, ly: usize, floor: usize) -> usize {
        (floor * BLOCK_HEIGHT + ly) * BLOCK_WIDTH + lx
    }

    #[inline]
    fn cell_at(&self, lx: usize, ly: usize, floor: usize) -> &Cell {
        &self.cells[Self::idx(lx, ly, floor)]
    }

    #[inline]
    fn cell_mut(&mut self, lx: usize, ly: usize, floor: usize) -> &mut Cell {
        &mut self.cells[Self::idx(lx, ly, floor)]
    }

    #[inline]
    fn local_to_world(&self, lx: usize, ly: usize, floor: Floor) -> Tile3D {
        Tile3D::new(self.origin.x + lx as i32, self.origin.y + ly as i32, floor)
    }

    #[inline]
    fn world_to_local(&self, tile: Tile3D) -> Option<(usize, usize, usize)> {
        if tile.floor as usize >= NUM_FLOORS {
            return None;
        }
        let lx = tile.x - self.origin.x;
        let ly = tile.y - self.origin.y;
        if !(0..BLOCK_WIDTH as i32).contains(&lx) || !(0..BLOCK_HEIGHT as i32).contains(&ly) {
            return None;
        }
        Some((lx as usize, ly as usize, tile.floor as usize))
    }

    fn stamp_exterior(&mut self) {
        for (ly, row) in EXTERIOR_MAP.iter().enumerate() {
            debug_assert_eq!(row.len(), BLOCK_WIDTH);
            for (lx, ch) in row.bytes().enumerate() {
                let cell = self.cell_mut(lx, ly, 0);
                let (surface, kind) = match ch {
                    b'R' => (Surface::Road, TileKind::Empty),
                    b's' => (Surface::Sidewalk, TileKind::Empty),
                    b'd' => (Surface::Driveway, TileKind::Empty),
                    b'p' => (Surface::Porch, TileKind::Empty),
                    b'g' => (Surface::Gravel, TileKind::Empty),
                    b'.' => (Surface::Grass, TileKind::Empty),
                    b'L' => (Surface::Sidewalk, TileKind::Streetlight),
                    b'T' => (Surface::Grass, TileKind::Prop(PropKind::BirchTree)),
                    b'f' => (Surface::Grass, TileKind::Prop(PropKind::Fence)),
                    b'b' => (Surface::Grass, TileKind::Prop(PropKind::Bench)),
                    b'm' => (Surface::Grass, TileKind::Prop(PropKind::Mailbox)),
                    b'M' => (Surface::Sidewalk, TileKind::Prop(PropKind::Mailbox)),
                    b'c' => (Surface::Grass, TileKind::Prop(PropKind::Crate)),
                    b'S' => (Surface::Gravel, TileKind::Prop(PropKind::Shed)),
                    other => panic!("unexpected exterior blueprint byte: {other}"),
                };
                cell.surface = surface;
                cell.set_kind(kind, Material::None);
            }
        }
    }

    fn stamp_houses(&mut self) {
        for bp in &HOUSE_BLUEPRINTS {
            self.buildings.push(BuildingInfo {
                id: bp.id,
                name: bp.name,
                x: self.origin.x + bp.x as i32,
                y: self.origin.y + bp.y as i32,
                width: bp.width as i32,
                height: bp.height as i32,
                floors: NUM_FLOORS as u8,
                style: bp.style,
                front_door: Position::new(
                    self.origin.x + bp.front_door.0 as i32,
                    self.origin.y + bp.front_door.1 as i32,
                ),
            });

            for (floor, grid) in [bp.f0, bp.f1].iter().enumerate() {
                debug_assert_eq!(grid.len(), bp.height);
                for (by, row) in grid.iter().enumerate() {
                    debug_assert_eq!(row.len(), bp.width);
                    let ly = bp.y + by;
                    let on_y_edge = by == 0 || by + 1 == bp.height;
                    for (bx, ch) in row.bytes().enumerate() {
                        let lx = bp.x + bx;
                        let on_x_edge = bx == 0 || bx + 1 == bp.width;
                        let on_perimeter = on_x_edge || on_y_edge;

                        let (surface, kind, wall_mat, load_bearing, stair_dir) = match ch {
                            b'P' => (
                                Surface::WoodFloor,
                                TileKind::Pillar,
                                bp.corner_mat,
                                floor == 0,
                                (0, 0),
                            ),
                            b'#' => (
                                Surface::WoodFloor,
                                TileKind::ExteriorWall,
                                bp.wall_mat,
                                floor == 0,
                                (0, 0),
                            ),
                            b'W' => (
                                Surface::WoodFloor,
                                TileKind::Window,
                                bp.wall_mat,
                                false,
                                (0, 0),
                            ),
                            b'O' => (
                                Surface::WoodFloor,
                                TileKind::Door {
                                    open: true,
                                    exterior: true,
                                },
                                bp.wall_mat,
                                false,
                                (0, 0),
                            ),
                            b'D' => (
                                Surface::WoodFloor,
                                TileKind::Door {
                                    open: false,
                                    exterior: true,
                                },
                                bp.wall_mat,
                                false,
                                (0, 0),
                            ),
                            b'i' => (
                                Surface::WoodFloor,
                                TileKind::InteriorWall,
                                Material::Drywall,
                                false,
                                (0, 0),
                            ),
                            b'd' => (
                                Surface::WoodFloor,
                                TileKind::Door {
                                    open: false,
                                    exterior: false,
                                },
                                Material::WoodDoor,
                                false,
                                (0, 0),
                            ),
                            b'U' => (
                                Surface::WoodFloor,
                                TileKind::StairsUp,
                                Material::None,
                                false,
                                (-1, 0),
                            ),
                            b'V' => (
                                Surface::WoodFloor,
                                TileKind::StairsDown,
                                Material::None,
                                false,
                                (-1, 0),
                            ),
                            b'.' => (
                                Surface::WoodFloor,
                                TileKind::Empty,
                                Material::None,
                                false,
                                (0, 0),
                            ),
                            b'k' => (
                                Surface::KitchenTile,
                                TileKind::Empty,
                                Material::None,
                                false,
                                (0, 0),
                            ),
                            b',' => (
                                Surface::Carpet,
                                TileKind::Empty,
                                Material::None,
                                false,
                                (0, 0),
                            ),
                            b'c' => (
                                Surface::KitchenTile,
                                TileKind::Prop(PropKind::Counter),
                                Material::None,
                                false,
                                (0, 0),
                            ),
                            b't' => (
                                Surface::WoodFloor,
                                TileKind::Prop(PropKind::Table),
                                Material::None,
                                false,
                                (0, 0),
                            ),
                            b'q' => (
                                Surface::KitchenTile,
                                TileKind::Prop(PropKind::Table),
                                Material::None,
                                false,
                                (0, 0),
                            ),
                            b's' => (
                                Surface::WoodFloor,
                                TileKind::Prop(PropKind::Sofa),
                                Material::None,
                                false,
                                (0, 0),
                            ),
                            b'b' => (
                                Surface::Carpet,
                                TileKind::Prop(PropKind::Bed),
                                Material::None,
                                false,
                                (0, 0),
                            ),
                            other => panic!("unexpected house blueprint byte: {other}"),
                        };

                        let cell = self.cell_mut(lx, ly, floor);
                        cell.surface = surface;
                        cell.set_kind(kind, wall_mat);
                        cell.building_id = bp.id;
                        cell.indoor = !on_perimeter;
                        cell.load_bearing = load_bearing;
                        cell.stair_dir = stair_dir;
                    }
                }
            }
        }
    }

    fn init_lights(&mut self) {
        for &(lx, ly) in &STREETLIGHT_POSITIONS {
            self.static_lights.push(StaticLight {
                lx,
                ly,
                floor: 0,
                radius: STREETLIGHT_RADIUS,
                intensity: STREETLIGHT_INTENSITY,
                active: true,
                kind: LightKind::Streetlight,
            });
        }
        for &(lx, ly) in &PORCH_LIGHT_POSITIONS {
            self.static_lights.push(StaticLight {
                lx,
                ly,
                floor: 0,
                radius: PORCH_LIGHT_RADIUS,
                intensity: PORCH_LIGHT_INTENSITY,
                active: true,
                kind: LightKind::Porch,
            });
        }
    }

    fn init_caches(&mut self) {
        struct CacheSpec {
            id: u8,
            lx: usize,
            ly: usize,
            floor: Floor,
            kind: CacheKind,
            loot: CacheLoot,
        }

        let specs = [
            CacheSpec {
                id: 0,
                lx: 10,
                ly: 24,
                floor: 0,
                kind: CacheKind::Weapon,
                loot: CacheLoot {
                    weapon: Some(Weapon::Axe),
                    ..CacheLoot::default()
                },
            },
            CacheSpec {
                id: 1,
                lx: 7,
                ly: 20,
                floor: 0,
                kind: CacheKind::Weapon,
                loot: CacheLoot {
                    weapon: Some(Weapon::Pistol),
                    pistol_ammo: 12,
                    ..CacheLoot::default()
                },
            },
            CacheSpec {
                id: 2,
                lx: 11,
                ly: 17,
                floor: 1,
                kind: CacheKind::Mixed,
                loot: CacheLoot {
                    medkits: 1,
                    pistol_ammo: 8,
                    ..CacheLoot::default()
                },
            },
            CacheSpec {
                id: 3,
                lx: 26,
                ly: 17,
                floor: 0,
                kind: CacheKind::Weapon,
                loot: CacheLoot {
                    weapon: Some(Weapon::Shotgun),
                    shotgun_ammo: 10,
                    ..CacheLoot::default()
                },
            },
            CacheSpec {
                id: 4,
                lx: 22,
                ly: 21,
                floor: 1,
                kind: CacheKind::Ammo,
                loot: CacheLoot {
                    pistol_ammo: 8,
                    shotgun_ammo: 6,
                    ..CacheLoot::default()
                },
            },
            CacheSpec {
                id: 5,
                lx: 8,
                ly: 4,
                floor: 1,
                kind: CacheKind::Weapon,
                loot: CacheLoot {
                    weapon: Some(Weapon::RocketLauncher),
                    rockets: 4,
                    ..CacheLoot::default()
                },
            },
            CacheSpec {
                id: 6,
                lx: 13,
                ly: 4,
                floor: 0,
                kind: CacheKind::Mixed,
                loot: CacheLoot {
                    medkits: 1,
                    pistol_ammo: 8,
                    ..CacheLoot::default()
                },
            },
            CacheSpec {
                id: 7,
                lx: 26,
                ly: 5,
                floor: 0,
                kind: CacheKind::Mixed,
                loot: CacheLoot {
                    shotgun_ammo: 6,
                    medkits: 1,
                    ..CacheLoot::default()
                },
            },
        ];

        for spec in specs {
            self.caches.push(SupplyCache {
                id: spec.id,
                pos: self.local_to_world(spec.lx, spec.ly, spec.floor),
                kind: spec.kind,
                loot: spec.loot,
                opened: false,
            });
        }
    }

    fn init_monster_spawns(&mut self) {
        let specs: [((usize, usize, Floor), MonsterKind, u8); 12] = [
            ((17, 21, 0), MonsterKind::Shambler, 0),
            ((3, 22, 0), MonsterKind::Shambler, 1),
            ((11, 18, 0), MonsterKind::Shambler, 2),
            ((7, 19, 1), MonsterKind::Brisk, 0),
            ((21, 27, 0), MonsterKind::Shambler, 3),
            ((25, 22, 0), MonsterKind::Brisk, 1),
            ((26, 18, 0), MonsterKind::Bloater, 0),
            ((27, 21, 1), MonsterKind::Shooter, 0),
            ((12, 11, 0), MonsterKind::Brisk, 2),
            ((9, 7, 0), MonsterKind::Bloater, 1),
            ((9, 4, 1), MonsterKind::Shooter, 1),
            ((24, 6, 0), MonsterKind::Shooter, 2),
        ];

        for ((lx, ly, floor), kind, variant) in specs {
            self.monster_spawns.push(MonsterSpawn {
                pos: self.local_to_world(lx, ly, floor),
                kind,
                variant,
            });
        }
    }

    fn apply_seeded_scatter(&mut self, seed: u64) {
        let spawn = self.player_spawn();
        for ly in 1..BLOCK_HEIGHT - 1 {
            for lx in 1..BLOCK_WIDTH - 1 {
                let cell = *self.cell_at(lx, ly, 0);
                if cell.building_id != 0
                    || cell.surface != Surface::Grass
                    || cell.kind != TileKind::Empty
                {
                    continue;
                }
                let h = mix64(seed ^ ((lx as u64) << 16) ^ (ly as u64) ^ 0x51a7_7e01);
                if h.is_multiple_of(9) {
                    self.cell_mut(lx, ly, 0).surface = Surface::Gravel;
                    continue;
                }
                // Place occasional yard crates strictly away from houses, doors, windows,
                // caches, spawns, and narrow passages.
                if h.is_multiple_of(43) && self.is_safe_scatter_prop_tile(lx, ly, spawn) {
                    self.cell_mut(lx, ly, 0)
                        .set_kind(TileKind::Prop(PropKind::Crate), Material::None);
                }
            }
        }
    }

    fn is_safe_scatter_prop_tile(&self, lx: usize, ly: usize, spawn: Tile3D) -> bool {
        let world_tile = self.local_to_world(lx, ly, 0);
        if world_tile.x.abs_diff(spawn.x) <= 2 && world_tile.y.abs_diff(spawn.y) <= 2 {
            return false;
        }
        for bp in &HOUSE_BLUEPRINTS {
            let min_x = bp.x.saturating_sub(2);
            let max_x = bp.x + bp.width + 1;
            let min_y = bp.y.saturating_sub(2);
            let max_y = bp.y + bp.height + 1;
            if (min_x..=max_x).contains(&lx) && (min_y..=max_y).contains(&ly) {
                return false;
            }
        }
        for cache in &self.caches {
            if cache.pos.floor == 0
                && cache.pos.x.abs_diff(world_tile.x) <= 2
                && cache.pos.y.abs_diff(world_tile.y) <= 2
            {
                return false;
            }
        }
        for m in &self.monster_spawns {
            if m.pos.floor == 0
                && m.pos.x.abs_diff(world_tile.x) <= 2
                && m.pos.y.abs_diff(world_tile.y) <= 2
            {
                return false;
            }
        }
        // Require all 8 neighbors to be open walkable grass so scatter never narrows or blocks paths.
        for dy in -1..=1_i32 {
            for dx in -1..=1_i32 {
                let nx = (lx as i32 + dx) as usize;
                let ny = (ly as i32 + dy) as usize;
                let n = self.cell_at(nx, ny, 0);
                if n.kind != TileKind::Empty
                    || !matches!(n.surface, Surface::Grass | Surface::Gravel)
                {
                    return false;
                }
            }
        }
        true
    }

    fn recompute_rooms(&mut self) {
        for cell in &mut self.cells {
            cell.room_id = 0;
        }
        let mut next_room = 1u16;
        let mut queue = VecDeque::new();
        const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

        for fl in 0..NUM_FLOORS {
            for ly in 0..BLOCK_HEIGHT {
                for lx in 0..BLOCK_WIDTH {
                    let start = self.cells[Self::idx(lx, ly, fl)];
                    if !start.indoor
                        || start.building_id == 0
                        || start.room_id != 0
                        || is_room_barrier(start.kind)
                    {
                        continue;
                    }
                    let room_id = next_room;
                    next_room = next_room.saturating_add(1);
                    self.cells[Self::idx(lx, ly, fl)].room_id = room_id;
                    queue.push_back((lx, ly));

                    while let Some((cx, cy)) = queue.pop_front() {
                        for (dx, dy) in DIRS {
                            let nx = cx as i32 + dx;
                            let ny = cy as i32 + dy;
                            if !(0..BLOCK_WIDTH as i32).contains(&nx)
                                || !(0..BLOCK_HEIGHT as i32).contains(&ny)
                            {
                                continue;
                            }
                            let (ux, uy) = (nx as usize, ny as usize);
                            let n = &mut self.cells[Self::idx(ux, uy, fl)];
                            if n.indoor
                                && n.building_id == start.building_id
                                && n.room_id == 0
                                && !is_room_barrier(n.kind)
                            {
                                n.room_id = room_id;
                                queue.push_back((ux, uy));
                            }
                        }
                    }
                }
            }
        }
    }

    fn wall_orientation(&self, lx: usize, ly: usize, fl: usize) -> WallOrientation {
        let cell = self.cell_at(lx, ly, fl);
        match cell.kind {
            TileKind::Pillar => WallOrientation::Corner,
            TileKind::ExteriorWall
            | TileKind::InteriorWall
            | TileKind::Window
            | TileKind::BrokenWindow
            | TileKind::Door { .. } => {
                let same_wall = |nx: i32, ny: i32| -> bool {
                    if !(0..BLOCK_WIDTH as i32).contains(&nx)
                        || !(0..BLOCK_HEIGHT as i32).contains(&ny)
                    {
                        return false;
                    }
                    let other = self.cell_at(nx as usize, ny as usize, fl);
                    other.building_id == cell.building_id && is_room_barrier(other.kind)
                };
                let has_x =
                    same_wall(lx as i32 - 1, ly as i32) || same_wall(lx as i32 + 1, ly as i32);
                let has_y =
                    same_wall(lx as i32, ly as i32 - 1) || same_wall(lx as i32, ly as i32 + 1);
                match (has_x, has_y) {
                    (true, true) => WallOrientation::Corner,
                    (false, true) => WallOrientation::AlongY,
                    _ => WallOrientation::AlongX,
                }
            }
            TileKind::Prop(PropKind::Fence) => {
                let is_fence = |nx: i32, ny: i32| -> bool {
                    if !(0..BLOCK_WIDTH as i32).contains(&nx)
                        || !(0..BLOCK_HEIGHT as i32).contains(&ny)
                    {
                        return false;
                    }
                    self.cell_at(nx as usize, ny as usize, fl).kind
                        == TileKind::Prop(PropKind::Fence)
                };
                let has_x =
                    is_fence(lx as i32 - 1, ly as i32) || is_fence(lx as i32 + 1, ly as i32);
                let has_y =
                    is_fence(lx as i32, ly as i32 - 1) || is_fence(lx as i32, ly as i32 + 1);
                match (has_x, has_y) {
                    (true, true) => WallOrientation::Corner,
                    (false, true) => WallOrientation::AlongY,
                    _ => WallOrientation::AlongX,
                }
            }
            _ => WallOrientation::AlongX,
        }
    }

    fn ensure_static_light(&self) {
        let mut cache = self.light_cache.borrow_mut();
        if cache.version == self.terrain_version {
            return;
        }
        let total = self.cells.len();
        let mut accum = vec![0.0_f32; total];
        for (i, cell) in self.cells.iter().enumerate() {
            accum[i] = if cell.indoor {
                INDOOR_AMBIENT
            } else {
                OUTDOOR_AMBIENT
            };
        }

        for light in &self.static_lights {
            if !light.active || light.intensity == 0 || light.radius <= 0.0 {
                continue;
            }
            let r_ceil = light.radius.ceil() as i32;
            let min_x = (light.lx as i32 - r_ceil).max(0) as usize;
            let max_x = (light.lx as i32 + r_ceil).min(BLOCK_WIDTH as i32 - 1) as usize;
            let min_y = (light.ly as i32 - r_ceil).max(0) as usize;
            let max_y = (light.ly as i32 + r_ceil).min(BLOCK_HEIGHT as i32 - 1) as usize;
            let fl = light.floor as usize;
            let src_closed_door = matches!(
                self.cell_at(light.lx, light.ly, fl).kind,
                TileKind::Door { open: false, .. }
            );

            for ly in min_y..=max_y {
                for lx in min_x..=max_x {
                    let dx = lx as f32 - light.lx as f32;
                    let dy = ly as f32 - light.ly as f32;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist >= light.radius {
                        continue;
                    }
                    let falloff = (1.0 - dist / light.radius).powf(1.35);

                    // Floor 0 illumination
                    let target_f0 = self.cell_at(lx, ly, fl);
                    if !(src_closed_door && target_f0.indoor) {
                        let transmit = self.light_transmittance(light.lx, light.ly, lx, ly, fl);
                        if transmit > 0.0 {
                            accum[Self::idx(lx, ly, fl)] +=
                                light.intensity as f32 * falloff * transmit;
                        }
                    }

                    // Active streetlights cast 0.55x bleed onto F1 windows & adjacent F1 tiles.
                    if light.kind == LightKind::Streetlight && fl == 0 && NUM_FLOORS > 1 {
                        let f1_cell = self.cell_at(lx, ly, 1);
                        let eligible =
                            matches!(f1_cell.kind, TileKind::Window | TileKind::BrokenWindow)
                                || f1_cell.surface == Surface::Air
                                || f1_cell.indoor;
                        if eligible {
                            let transmit_f1 =
                                self.light_transmittance(light.lx, light.ly, lx, ly, 1);
                            if transmit_f1 > 0.0 {
                                accum[Self::idx(lx, ly, 1)] += light.intensity as f32
                                    * STREETLIGHT_F1_BLEED
                                    * falloff
                                    * transmit_f1;
                            }
                        }
                    }
                }
            }
        }

        for (out, val) in cache.levels.iter_mut().zip(accum) {
            *out = val.round().clamp(0.0, 255.0) as u8;
        }
        cache.version = self.terrain_version;
    }

    fn light_transmittance(&self, sx: usize, sy: usize, tx: usize, ty: usize, fl: usize) -> f32 {
        if sx == tx && sy == ty {
            return 1.0;
        }
        let direct = self.trace_light_ray(sx, sy, tx, ty, fl);
        if direct > 0.0 {
            return direct;
        }
        // Windows sit flush on the exterior facade: if the light is on the exterior side
        // of the window wall and reaches the open cell directly outside the pane, it illuminates it.
        if matches!(
            self.cell_at(tx, ty, fl).kind,
            TileKind::Window | TileKind::BrokenWindow
        ) {
            const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
            let ldx = sx as i32 - tx as i32;
            let ldy = sy as i32 - ty as i32;
            let mut best = 0.0_f32;
            for (nx, ny) in DIRS {
                if nx * ldx + ny * ldy <= 0 {
                    continue;
                }
                let ox = tx as i32 + nx;
                let oy = ty as i32 + ny;
                if !(0..BLOCK_WIDTH as i32).contains(&ox) || !(0..BLOCK_HEIGHT as i32).contains(&oy)
                {
                    continue;
                }
                let (oux, ouy) = (ox as usize, oy as usize);
                if self.cell_at(oux, ouy, fl).building_id == 0
                    && self.light_cell_factor(ox, oy, fl) > 0.0
                {
                    best = best.max(self.trace_light_ray(sx, sy, oux, ouy, fl));
                }
            }
            return best;
        }
        0.0
    }

    fn trace_light_ray(&self, sx: usize, sy: usize, tx: usize, ty: usize, fl: usize) -> f32 {
        if sx == tx && sy == ty {
            return 1.0;
        }
        let mut transmittance = 1.0_f32;
        let from = Vec2::new(sx as f32, sy as f32);
        let to = Vec2::new(tx as f32, ty as f32);
        let clear = self.trace_grid_dda(from, to, |cx, cy, corner_a, corner_b| {
            if (cx == sx as i32 && cy == sy as i32) || (cx == tx as i32 && cy == ty as i32) {
                return true;
            }
            if let (Some((ax, ay)), Some((bx, by))) = (corner_a, corner_b) {
                if self.light_cell_factor(ax, ay, fl) <= 0.0
                    && self.light_cell_factor(bx, by, fl) <= 0.0
                {
                    transmittance = 0.0;
                    return false;
                }
            }
            let f = self.light_cell_factor(cx, cy, fl);
            transmittance *= f;
            transmittance > 0.001
        });
        if clear { transmittance } else { 0.0 }
    }

    fn light_cell_factor(&self, lx: i32, ly: i32, fl: usize) -> f32 {
        if !(0..BLOCK_WIDTH as i32).contains(&lx) || !(0..BLOCK_HEIGHT as i32).contains(&ly) {
            return 0.0;
        }
        match self.cell_at(lx as usize, ly as usize, fl).kind {
            TileKind::ExteriorWall
            | TileKind::InteriorWall
            | TileKind::Pillar
            | TileKind::Door { open: false, .. }
            | TileKind::Prop(PropKind::Shed) => 0.0,
            TileKind::Window => WINDOW_LIGHT_TRANSMIT,
            TileKind::Prop(PropKind::BirchTree) => TREE_LIGHT_TRANSMIT,
            _ => 1.0,
        }
    }

    fn los_reaches(&self, viewer_local: Vec2, tx: usize, ty: usize, fl: usize) -> bool {
        let vx = viewer_local.x.round() as i32;
        let vy = viewer_local.y.round() as i32;
        if vx == tx as i32 && vy == ty as i32 {
            return true;
        }
        let to = Vec2::new(tx as f32, ty as f32);
        self.trace_grid_dda(viewer_local, to, |cx, cy, corner_a, corner_b| {
            if let (Some((ax, ay)), Some((bx, by))) = (corner_a, corner_b) {
                if self.is_los_opaque(ax, ay, fl) && self.is_los_opaque(bx, by, fl) {
                    return false;
                }
            }
            if cx == tx as i32 && cy == ty as i32 {
                return true;
            }
            if cx == vx && cy == vy {
                return true;
            }
            !self.is_los_opaque(cx, cy, fl)
        })
    }

    fn is_los_opaque(&self, lx: i32, ly: i32, fl: usize) -> bool {
        if !(0..BLOCK_WIDTH as i32).contains(&lx) || !(0..BLOCK_HEIGHT as i32).contains(&ly) {
            return true;
        }
        matches!(
            self.cell_at(lx as usize, ly as usize, fl).kind,
            TileKind::ExteriorWall
                | TileKind::InteriorWall
                | TileKind::Pillar
                | TileKind::Door { open: false, .. }
                | TileKind::Prop(PropKind::Shed)
                | TileKind::Prop(PropKind::BirchTree)
        )
    }

    /// 2D Amanatides & Woo grid DDA in tile-center coordinates (tile `(x, y)` spans `[x-0.5, x+0.5]`).
    /// Calls `visit(cx, cy, corner_a, corner_b)` on every stepped cell after start; stops and returns
    /// `false` if `visit` returns `false`.
    fn trace_grid_dda<F>(&self, from: Vec2, to: Vec2, mut visit: F) -> bool
    where
        F: FnMut(i32, i32, Option<(i32, i32)>, Option<(i32, i32)>) -> bool,
    {
        let fx = from.x + 0.5;
        let fy = from.y + 0.5;
        let tx = to.x + 0.5;
        let ty = to.y + 0.5;

        let mut cx = fx.floor() as i32;
        let mut cy = fy.floor() as i32;
        let end_x = tx.floor() as i32;
        let end_y = ty.floor() as i32;

        if cx == end_x && cy == end_y {
            return true;
        }

        let dx = tx - fx;
        let dy = ty - fy;
        let step_x = if dx > 0.0 {
            1
        } else if dx < 0.0 {
            -1
        } else {
            0
        };
        let step_y = if dy > 0.0 {
            1
        } else if dy < 0.0 {
            -1
        } else {
            0
        };

        let t_delta_x = if step_x != 0 {
            (1.0 / dx).abs()
        } else {
            f32::INFINITY
        };
        let t_delta_y = if step_y != 0 {
            (1.0 / dy).abs()
        } else {
            f32::INFINITY
        };

        let mut t_max_x = if step_x > 0 {
            ((cx as f32 + 1.0) - fx) / dx
        } else if step_x < 0 {
            (cx as f32 - fx) / dx
        } else {
            f32::INFINITY
        };
        let mut t_max_y = if step_y > 0 {
            ((cy as f32 + 1.0) - fy) / dy
        } else if step_y < 0 {
            (cy as f32 - fy) / dy
        } else {
            f32::INFINITY
        };

        let max_steps = (BLOCK_WIDTH + BLOCK_HEIGHT) * 2;
        for _ in 0..max_steps {
            if cx == end_x && cy == end_y {
                return true;
            }
            let diff = t_max_x - t_max_y;
            let (corner_a, corner_b) = if diff.abs() < 1e-4 {
                let a = (cx + step_x, cy);
                let b = (cx, cy + step_y);
                cx += step_x;
                cy += step_y;
                t_max_x += t_delta_x;
                t_max_y += t_delta_y;
                (Some(a), Some(b))
            } else if diff < 0.0 {
                cx += step_x;
                t_max_x += t_delta_x;
                (None, None)
            } else {
                cy += step_y;
                t_max_y += t_delta_y;
                (None, None)
            };

            if !visit(cx, cy, corner_a, corner_b) {
                return false;
            }
        }
        true
    }

    fn count_f0_load_bearing_3x3(&self, lx: usize, ly: usize) -> u8 {
        let mut count = 0u8;
        for dy in -1..=1_i32 {
            for dx in -1..=1_i32 {
                let nx = lx as i32 + dx;
                let ny = ly as i32 + dy;
                if (0..BLOCK_WIDTH as i32).contains(&nx)
                    && (0..BLOCK_HEIGHT as i32).contains(&ny)
                    && self.cell_at(nx as usize, ny as usize, 0).load_bearing
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// Deterministic 2-pass Floor 0 -> Floor 1 structural overhang collapse.
    fn resolve_structural_collapse(&mut self, breached_lx: usize, breached_ly: usize) -> u8 {
        const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        let mut pass1: Vec<(usize, usize)> = Vec::new();

        // Pass 1: check F1 cell directly above and 4-adjacent to the destroyed F0 support.
        let candidates = [
            (breached_lx as i32, breached_ly as i32),
            (breached_lx as i32, breached_ly as i32 - 1),
            (breached_lx as i32 + 1, breached_ly as i32),
            (breached_lx as i32, breached_ly as i32 + 1),
            (breached_lx as i32 - 1, breached_ly as i32),
        ];
        for (cx, cy) in candidates {
            if !(0..BLOCK_WIDTH as i32).contains(&cx) || !(0..BLOCK_HEIGHT as i32).contains(&cy) {
                continue;
            }
            let (ux, uy) = (cx as usize, cy as usize);
            let f1 = self.cell_at(ux, uy, 1);
            if f1.surface == Surface::Air || f1.kind == TileKind::StairsDown {
                continue;
            }
            if self.count_f0_load_bearing_3x3(ux, uy) < 2 {
                pass1.push((ux, uy));
            }
        }

        if pass1.is_empty() {
            return 0;
        }

        for &(ux, uy) in &pass1 {
            self.collapse_f1_cell(ux, uy);
        }

        // Pass 2: 1-ring cascade into F1 neighbors with >= 2 collapsed 4-neighbors.
        let mut pass2: Vec<(usize, usize)> = Vec::new();
        for &(ux, uy) in &pass1 {
            for (dx, dy) in DIRS {
                let nx = ux as i32 + dx;
                let ny = uy as i32 + dy;
                if !(0..BLOCK_WIDTH as i32).contains(&nx) || !(0..BLOCK_HEIGHT as i32).contains(&ny)
                {
                    continue;
                }
                let (nxu, nyu) = (nx as usize, ny as usize);
                if pass2.contains(&(nxu, nyu)) {
                    continue;
                }
                let f1 = self.cell_at(nxu, nyu, 1);
                if f1.surface == Surface::Air || f1.kind == TileKind::StairsDown {
                    continue;
                }
                let mut collapsed_neighbors = 0u8;
                for (adx, ady) in DIRS {
                    let ax = nx + adx;
                    let ay = ny + ady;
                    if (0..BLOCK_WIDTH as i32).contains(&ax)
                        && (0..BLOCK_HEIGHT as i32).contains(&ay)
                        && self.cell_at(ax as usize, ay as usize, 1).collapsed
                    {
                        collapsed_neighbors += 1;
                    }
                }
                if collapsed_neighbors >= 2 {
                    pass2.push((nxu, nyu));
                }
            }
        }

        for &(ux, uy) in &pass2 {
            self.collapse_f1_cell(ux, uy);
        }

        (pass1.len() + pass2.len()).min(u8::MAX as usize) as u8
    }

    fn collapse_f1_cell(&mut self, lx: usize, ly: usize) {
        let f1 = self.cell_mut(lx, ly, 1);
        f1.surface = Surface::Air;
        f1.set_kind(TileKind::Empty, Material::None);
        f1.indoor = false;
        f1.load_bearing = false;
        f1.collapsed = true;
        f1.room_id = 0;

        let f0 = self.cell_mut(lx, ly, 0);
        if !f0.load_bearing && !matches!(f0.kind, TileKind::StairsUp | TileKind::Door { .. }) {
            f0.set_kind(TileKind::Rubble, Material::None);
        }

        let f0_world = self.local_to_world(lx, ly, 0);
        let f1_world = self.local_to_world(lx, ly, 1);
        for cache in &mut self.caches {
            if !cache.opened && cache.pos == f1_world {
                cache.pos = f0_world;
            }
        }
        self.pending_collapses.push(f0_world);
    }
}

impl CombatWorld for ResidentialBlock {
    fn bounds(&self) -> (Position, Position, u8) {
        (
            self.origin,
            Position::new(
                self.origin.x + BLOCK_WIDTH as i32 - 1,
                self.origin.y + BLOCK_HEIGHT as i32 - 1,
            ),
            NUM_FLOORS as u8,
        )
    }

    fn terrain_version(&self) -> u64 {
        self.terrain_version
    }

    fn obstruction(&self, tile: Tile3D) -> TileObstruction {
        let Some((lx, ly, fl)) = self.world_to_local(tile) else {
            return TileObstruction::Wall(Material::BrickMasonry);
        };
        let cell = self.cell_at(lx, ly, fl);
        match cell.kind {
            TileKind::Empty
            | TileKind::Rubble
            | TileKind::StairsUp
            | TileKind::StairsDown
            | TileKind::BrokenStreetlight
            | TileKind::Door { open: true, .. } => TileObstruction::Open,
            TileKind::BrokenWindow => TileObstruction::BrokenWindow,
            TileKind::Window => TileObstruction::IntactWindow,
            TileKind::Door { open: false, .. } => TileObstruction::DoorClosed,
            TileKind::Streetlight => TileObstruction::LowProp(cell.material),
            TileKind::Prop(PropKind::Shed | PropKind::BirchTree) => {
                TileObstruction::Wall(cell.material)
            }
            TileKind::Prop(_) => TileObstruction::LowProp(cell.material),
            TileKind::ExteriorWall | TileKind::InteriorWall | TileKind::Pillar => {
                TileObstruction::Wall(cell.material)
            }
        }
    }

    fn has_floor(&self, tile: Tile3D) -> bool {
        self.world_to_local(tile)
            .is_some_and(|(lx, ly, fl)| self.cell_at(lx, ly, fl).surface != Surface::Air)
    }

    fn is_walkable(&self, tile: Tile3D, can_vault_window: bool) -> bool {
        if !self.has_floor(tile) {
            return false;
        }
        match self.obstruction(tile) {
            TileObstruction::Open => true,
            TileObstruction::BrokenWindow => can_vault_window && tile.floor == 0,
            _ => false,
        }
    }

    fn building_id(&self, tile: Tile3D) -> u8 {
        self.world_to_local(tile)
            .map_or(0, |(lx, ly, fl)| self.cell_at(lx, ly, fl).building_id)
    }

    fn stair_link(&self, tile: Tile3D) -> Option<(Tile3D, (i8, i8))> {
        let (lx, ly, fl) = self.world_to_local(tile)?;
        let cell = self.cell_at(lx, ly, fl);
        match (fl, cell.kind) {
            (0, TileKind::StairsUp) if self.cell_at(lx, ly, 1).kind == TileKind::StairsDown => {
                Some((Tile3D::new(tile.x, tile.y, 1), cell.stair_dir))
            }
            (1, TileKind::StairsDown) if self.cell_at(lx, ly, 0).kind == TileKind::StairsUp => {
                Some((Tile3D::new(tile.x, tile.y, 0), cell.stair_dir))
            }
            _ => None,
        }
    }

    fn window_drop_target(&self, tile: Tile3D) -> Option<Tile3D> {
        if tile.floor != 1 {
            return None;
        }
        let (lx, ly, _) = self.world_to_local(tile)?;
        let cell = self.cell_at(lx, ly, 1);

        if cell.collapsed {
            let below = Tile3D::new(tile.x, tile.y, 0);
            return self.is_walkable(below, false).then_some(below);
        }

        if matches!(cell.kind, TileKind::BrokenWindow | TileKind::Rubble) {
            const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
            for (dx, dy) in DIRS {
                let adj_f1 = Tile3D::new(tile.x + dx, tile.y + dy, 1);
                if !self.has_floor(adj_f1) && self.building_id(adj_f1) == 0 {
                    let landing = Tile3D::new(tile.x + dx, tile.y + dy, 0);
                    if self.is_walkable(landing, false) {
                        return Some(landing);
                    }
                }
            }
        }
        None
    }

    fn window_vault_target(&self, from: Tile3D, window: Tile3D) -> Option<Tile3D> {
        if from.floor != 0 || window.floor != 0 {
            return None;
        }
        let (lx, ly, _) = self.world_to_local(window)?;
        if self.cell_at(lx, ly, 0).kind != TileKind::BrokenWindow {
            return None;
        }
        let dx = window.x - from.x;
        let dy = window.y - from.y;
        if dx.abs() + dy.abs() != 1 {
            return None;
        }
        let dest = Tile3D::new(window.x + dx, window.y + dy, 0);
        self.is_walkable(dest, false).then_some(dest)
    }

    fn light_level(&self, tile: Tile3D) -> f32 {
        self.effective_light_u8(tile) as f32 / 255.0
    }

    fn damage_tile(&mut self, tile: Tile3D, kind: DamageKind) -> Option<TileDamageResult> {
        let (lx, ly, fl) = self.world_to_local(tile)?;
        let cell = *self.cell_at(lx, ly, fl);
        if cell.hp == 0 || cell.material == Material::None {
            return None;
        }
        let dmg = damage_against(cell.material, kind);
        if dmg == 0 {
            return None;
        }

        let old_kind = cell.kind;
        let material = cell.material;
        let remaining = cell.hp.saturating_sub(dmg);
        self.cell_mut(lx, ly, fl).hp = remaining;

        if remaining > 0 {
            let sound_radius = match old_kind {
                TileKind::ExteriorWall | TileKind::InteriorWall | TileKind::Pillar => 4.5,
                _ => 3.5,
            };
            return Some(TileDamageResult {
                damaged: true,
                destroyed: false,
                material,
                old_kind,
                new_kind: old_kind,
                sound_radius,
                light_changed: false,
                collapsed_cells: 0,
            });
        }

        let was_load_bearing = cell.load_bearing;
        let (new_kind, sound_radius, light_changed) = match old_kind {
            TileKind::Window => (TileKind::BrokenWindow, 8.5, false),
            TileKind::Streetlight => (TileKind::BrokenStreetlight, 9.5, true),
            TileKind::Door { open, .. } => (TileKind::Rubble, 7.5, !open),
            TileKind::Prop(prop) => (
                TileKind::Rubble,
                7.5,
                matches!(prop, PropKind::Shed | PropKind::BirchTree),
            ),
            TileKind::InteriorWall | TileKind::ExteriorWall | TileKind::Pillar => {
                (TileKind::Rubble, 14.0, true)
            }
            other => (other, 0.0, false),
        };

        {
            let target = self.cell_mut(lx, ly, fl);
            target.set_kind(new_kind, Material::None);
            target.load_bearing = false;
        }

        if old_kind == TileKind::Streetlight {
            for light in &mut self.static_lights {
                if light.kind == LightKind::Streetlight
                    && light.lx == lx
                    && light.ly == ly
                    && light.floor == tile.floor
                {
                    light.active = false;
                }
            }
        }

        let mut collapsed_cells = 0u8;
        if fl == 0 && was_load_bearing {
            collapsed_cells = self.resolve_structural_collapse(lx, ly);
        }

        if matches!(
            old_kind,
            TileKind::ExteriorWall | TileKind::InteriorWall | TileKind::Pillar
        ) || collapsed_cells > 0
        {
            self.recompute_rooms();
        }

        self.terrain_version += 1;

        Some(TileDamageResult {
            damaged: true,
            destroyed: true,
            material,
            old_kind,
            new_kind,
            sound_radius,
            light_changed: light_changed || collapsed_cells > 0,
            collapsed_cells,
        })
    }

    fn toggle_door(&mut self, tile: Tile3D) -> Option<bool> {
        let (lx, ly, fl) = self.world_to_local(tile)?;
        let cell = self.cell_mut(lx, ly, fl);
        let TileKind::Door { open, exterior } = cell.kind else {
            return None;
        };
        let now_open = !open;
        cell.kind = TileKind::Door {
            open: now_open,
            exterior,
        };
        self.terrain_version += 1;
        Some(now_open)
    }

    fn register_dynamic_flash(
        &mut self,
        pos: Vec2,
        floor: Floor,
        radius: f32,
        intensity: u8,
        ttl_ticks: u8,
    ) {
        if ttl_ticks == 0 || intensity == 0 || radius <= 0.0 {
            return;
        }
        if self.flashes.len() >= MAX_DYNAMIC_FLASHES {
            self.flashes.remove(0);
        }
        self.flashes.push(DynamicFlash {
            pos,
            floor,
            radius,
            intensity,
            ttl_ticks,
        });
    }
}

fn is_room_barrier(kind: TileKind) -> bool {
    matches!(
        kind,
        TileKind::ExteriorWall
            | TileKind::InteriorWall
            | TileKind::Pillar
            | TileKind::Window
            | TileKind::BrokenWindow
            | TileKind::Door { .. }
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{HashSet, VecDeque};

    use super::*;
    use crate::Ground;

    fn block_for_seed(seed: u64) -> (World, ResidentialBlock) {
        let world = World::new(seed);
        let block = ResidentialBlock::new(&world, seed);
        (world, block)
    }

    #[test]
    fn roads_align_with_county_world_and_origin_matches_chunk() {
        for seed in [42, 7, 99] {
            let (world, block) = block_for_seed(seed);
            let wake = world.wake_position();
            let cx = wake.x.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
            let cy = wake.y.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
            assert_eq!(block.origin(), Position::new(cx - 9, cy - 19));

            for ly in 0..BLOCK_HEIGHT {
                for lx in 0..BLOCK_WIDTH {
                    let wp =
                        Position::new(block.origin().x + lx as i32, block.origin().y + ly as i32);
                    let info = block.tile_render_info(wp, 0).expect("in-bounds F0");
                    let world_tile = world.tile_at(wp).expect("in county bounds");
                    let expected_road = lx == 1 || lx == 17 || lx == 33 || ly == 11 || ly == 27;
                    assert_eq!(
                        info.surface == Surface::Road,
                        expected_road,
                        "local ({lx},{ly}) road mismatch"
                    );
                    assert_eq!(
                        world_tile.ground == Ground::Road,
                        expected_road,
                        "world ({},{}) road alignment mismatch (x%16={}, y%16={})",
                        wp.x,
                        wp.y,
                        wp.x.rem_euclid(CHUNK_SIZE),
                        wp.y.rem_euclid(CHUNK_SIZE),
                    );
                }
            }

            let starter = block.starter_house();
            assert_eq!(starter.x, block.origin().x + 5);
            assert_eq!(starter.y, block.origin().y + 16);
            assert_eq!(starter.width, 9);
            assert_eq!(starter.height, 8);
            assert_eq!(
                starter.door,
                Position::new(block.origin().x + 9, block.origin().y + 23)
            );
            assert_eq!(
                block.player_spawn(),
                Tile3D::new(block.origin().x + 9, block.origin().y + 25, 0)
            );
            for &(lx, ly) in &STREETLIGHT_POSITIONS {
                let tile = block.local_to_world(lx, ly, 0);
                assert_eq!(
                    block.tile_kind(tile),
                    TileKind::Streetlight,
                    "expected Streetlight at local ({lx},{ly})"
                );
            }
            assert_eq!(
                block
                    .light_sources()
                    .iter()
                    .filter(|l| l.kind == LightKind::Streetlight)
                    .count(),
                6
            );
            assert_eq!(
                block
                    .light_sources()
                    .iter()
                    .filter(|l| l.kind == LightKind::Porch)
                    .count(),
                5
            );
            let (min, max, floors) = block.bounds();
            assert_eq!(min, block.origin());
            assert_eq!(
                max,
                Position::new(
                    block.origin().x + BLOCK_WIDTH as i32 - 1,
                    block.origin().y + BLOCK_HEIGHT as i32 - 1
                )
            );
            assert_eq!(floors, NUM_FLOORS as u8);
            assert_eq!(block.buildings().len(), 3);
            assert_eq!(block.buildings()[0].style, BuildingStyle::BirchSage);
            assert_eq!(block.buildings()[1].style, BuildingStyle::ColonialBrick);
            assert_eq!(block.buildings()[2].style, BuildingStyle::BlueClapboard);

            // House 3 has BrickMasonry corner pillars and WoodSiding exterior walls.
            let h3_corner = block.local_to_world(6, 2, 0);
            let h3_wall = block.local_to_world(7, 2, 0);
            assert_eq!(
                block.tile_render_info(h3_corner.pos(), 0).unwrap().material,
                Material::BrickMasonry
            );
            assert_eq!(
                block.tile_render_info(h3_wall.pos(), 0).unwrap().material,
                Material::WoodSiding
            );
            assert!(!block.is_indoor(block.player_spawn()));
            assert!(block.is_indoor(block.local_to_world(7, 20, 0)));
        }
    }

    #[test]
    fn stairs_windows_and_doors_are_structurally_valid() {
        for seed in [42, 7, 99] {
            let (_, mut block) = block_for_seed(seed);
            let mut stairs_up_count = 0usize;
            let mut stairs_down_count = 0usize;
            let mut f0_windows = 0usize;
            let mut f1_windows = 0usize;

            for fl in 0..NUM_FLOORS as u8 {
                for ly in 0..BLOCK_HEIGHT {
                    for lx in 0..BLOCK_WIDTH {
                        let tile = block.local_to_world(lx, ly, fl);
                        let kind = block.tile_kind(tile);
                        match kind {
                            TileKind::StairsUp => {
                                assert_eq!(fl, 0);
                                stairs_up_count += 1;
                                let (paired, dir) = block
                                    .stair_link(tile)
                                    .expect("StairsUp must link to StairsDown");
                                assert_eq!(paired, Tile3D::new(tile.x, tile.y, 1));
                                assert_eq!(dir, (-1, 0));
                                let landing_f0 =
                                    Tile3D::new(tile.x + dir.0 as i32, tile.y + dir.1 as i32, 0);
                                let landing_f1 =
                                    Tile3D::new(tile.x + dir.0 as i32, tile.y + dir.1 as i32, 1);
                                assert!(
                                    block.is_walkable(landing_f0, false),
                                    "F0 stair landing blocked"
                                );
                                assert!(
                                    block.is_walkable(landing_f1, false),
                                    "F1 stair landing blocked"
                                );
                            }
                            TileKind::StairsDown => {
                                assert_eq!(fl, 1);
                                stairs_down_count += 1;
                                let (paired, dir) = block
                                    .stair_link(tile)
                                    .expect("StairsDown must link to StairsUp");
                                assert_eq!(paired, Tile3D::new(tile.x, tile.y, 0));
                                assert_eq!(dir, (-1, 0));
                            }
                            TileKind::Door { .. } => {
                                let info = block.tile_render_info(tile.pos(), fl).unwrap();
                                let (a, b) = match info.orientation {
                                    WallOrientation::AlongX => (
                                        Tile3D::new(tile.x, tile.y - 1, fl),
                                        Tile3D::new(tile.x, tile.y + 1, fl),
                                    ),
                                    WallOrientation::AlongY => (
                                        Tile3D::new(tile.x - 1, tile.y, fl),
                                        Tile3D::new(tile.x + 1, tile.y, fl),
                                    ),
                                    WallOrientation::Corner => {
                                        panic!("door at local ({lx},{ly}) F{fl} cannot be a corner")
                                    }
                                };
                                assert!(
                                    block.is_walkable(a, false),
                                    "door at local ({lx},{ly}) F{fl} blocked on side A ({},{})",
                                    a.x - block.origin().x,
                                    a.y - block.origin().y
                                );
                                assert!(
                                    block.is_walkable(b, false),
                                    "door at local ({lx},{ly}) F{fl} blocked on side B ({},{})",
                                    b.x - block.origin().x,
                                    b.y - block.origin().y
                                );
                            }
                            TileKind::Window => {
                                let info = block.tile_render_info(tile.pos(), fl).unwrap();
                                let (a, b) = match info.orientation {
                                    WallOrientation::AlongX => (
                                        Tile3D::new(tile.x, tile.y - 1, fl),
                                        Tile3D::new(tile.x, tile.y + 1, fl),
                                    ),
                                    WallOrientation::AlongY => (
                                        Tile3D::new(tile.x - 1, tile.y, fl),
                                        Tile3D::new(tile.x + 1, tile.y, fl),
                                    ),
                                    WallOrientation::Corner => {
                                        panic!(
                                            "window at local ({lx},{ly}) F{fl} cannot be a corner"
                                        )
                                    }
                                };
                                if fl == 0 {
                                    f0_windows += 1;
                                    assert!(
                                        block.is_walkable(a, false),
                                        "F0 window ({lx},{ly}) blocked on side A"
                                    );
                                    assert!(
                                        block.is_walkable(b, false),
                                        "F0 window ({lx},{ly}) blocked on side B"
                                    );
                                } else {
                                    f1_windows += 1;
                                    let (inside, outside_f0) = if block.has_floor(a) {
                                        (a, Tile3D::new(b.x, b.y, 0))
                                    } else {
                                        (b, Tile3D::new(a.x, a.y, 0))
                                    };
                                    assert!(
                                        block.is_walkable(inside, false),
                                        "F1 window ({lx},{ly}) interior blocked"
                                    );
                                    assert!(
                                        block.is_walkable(outside_f0, false),
                                        "F1 window ({lx},{ly}) F0 drop tile blocked"
                                    );
                                    // Shattering F1 window must yield the exact F0 drop tile.
                                    let res = block
                                        .damage_tile(tile, DamageKind::Axe)
                                        .expect("shatter F1 window");
                                    assert!(res.destroyed);
                                    assert_eq!(block.window_drop_target(tile), Some(outside_f0));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            assert_eq!(stairs_up_count, 3);
            assert_eq!(stairs_down_count, 3);
            assert!(f0_windows >= 15);
            assert!(f1_windows >= 15);
        }
    }

    #[test]
    fn all_caches_and_monster_spawns_are_reachable_across_seeds() {
        for seed in [42, 7, 99] {
            let (_, block) = block_for_seed(seed);
            let spawn = block.player_spawn();
            assert!(block.is_walkable(spawn, false));
            assert_eq!(block.caches().len(), 8);
            assert_eq!(block.monster_spawns().len(), 12);

            let mut visited: HashSet<Tile3D> = HashSet::from([spawn]);
            let mut queue: VecDeque<Tile3D> = VecDeque::from([spawn]);
            const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

            while let Some(cur) = queue.pop_front() {
                for (dx, dy) in DIRS {
                    let next = Tile3D::new(cur.x + dx, cur.y + dy, cur.floor);
                    if visited.contains(&next) {
                        continue;
                    }
                    let passable = block.is_walkable(next, false)
                        || matches!(block.obstruction(next), TileObstruction::DoorClosed);
                    if passable && block.has_floor(next) {
                        visited.insert(next);
                        queue.push_back(next);
                    }
                }
                if let Some((paired, _)) = block.stair_link(cur) {
                    if visited.insert(paired) {
                        queue.push_back(paired);
                    }
                }
            }

            let mut cache_positions = HashSet::new();
            for cache in block.caches() {
                assert!(
                    cache_positions.insert(cache.pos),
                    "duplicate cache pos {:?}",
                    cache.pos
                );
                assert_ne!(cache.pos, spawn);
                assert!(
                    block.is_walkable(cache.pos, false),
                    "seed {seed}: cache {} at {:?} is not walkable",
                    cache.id,
                    cache.pos
                );
                assert!(
                    visited.contains(&cache.pos),
                    "seed {seed}: cache {} at {:?} is unreachable from spawn",
                    cache.id,
                    cache.pos
                );
            }

            let mut spawn_positions = HashSet::new();
            for (idx, m) in block.monster_spawns().iter().enumerate() {
                assert!(
                    spawn_positions.insert(m.pos),
                    "duplicate monster spawn pos {:?}",
                    m.pos
                );
                assert_ne!(m.pos, spawn);
                assert!(!cache_positions.contains(&m.pos));
                assert!(
                    block.is_walkable(m.pos, false),
                    "seed {seed}: monster spawn {idx} at {:?} is not walkable",
                    m.pos
                );
                assert!(
                    visited.contains(&m.pos),
                    "seed {seed}: monster spawn {idx} at {:?} is unreachable from spawn",
                    m.pos
                );
            }
        }
    }

    #[test]
    fn material_durability_matches_table_and_streetlight_darkens_corner() {
        let (_, mut block) = block_for_seed(42);

        // 1 axe hit shatters a window -> BrokenWindow (sound 8.5)
        let window = block.local_to_world(7, 23, 0);
        assert_eq!(block.tile_kind(window), TileKind::Window);
        let res = block.damage_tile(window, DamageKind::Axe).unwrap();
        assert!(res.damaged && res.destroyed);
        assert_eq!(res.material, Material::Glass);
        assert_eq!(res.new_kind, TileKind::BrokenWindow);
        assert!((res.sound_radius - 8.5).abs() < 0.01);

        // 3 axe hits break a door -> Rubble (sound 7.5)
        let door = block.local_to_world(11, 16, 0);
        assert!(matches!(block.tile_kind(door), TileKind::Door { .. }));
        assert!(!block.damage_tile(door, DamageKind::Axe).unwrap().destroyed);
        assert!(!block.damage_tile(door, DamageKind::Axe).unwrap().destroyed);
        let door_break = block.damage_tile(door, DamageKind::Axe).unwrap();
        assert!(door_break.destroyed);
        assert_eq!(door_break.new_kind, TileKind::Rubble);
        assert!((door_break.sound_radius - 7.5).abs() < 0.01);

        // 2 axe hits break a streetlight -> BrokenStreetlight and darken corner
        let lamp = block.local_to_world(16, 26, 0);
        let corner = block.local_to_world(16, 25, 0);
        let light_before = block.effective_light_u8(corner);
        assert!(!block.damage_tile(lamp, DamageKind::Axe).unwrap().destroyed);
        let lamp_break = block.damage_tile(lamp, DamageKind::Axe).unwrap();
        assert!(lamp_break.destroyed);
        assert!(lamp_break.light_changed);
        assert_eq!(lamp_break.new_kind, TileKind::BrokenStreetlight);
        assert!((lamp_break.sound_radius - 9.5).abs() < 0.01);
        let light_after = block.effective_light_u8(corner);
        assert!(
            light_after < light_before,
            "expected corner light to drop ({light_before} -> {light_after})"
        );
        assert!((block.light_level(corner) - light_after as f32 / 255.0).abs() < 1e-5);
        assert!(
            block
                .light_sources()
                .iter()
                .any(|l| l.kind == LightKind::Streetlight && !l.active)
        );

        // Streetlight (6,10) bleeds 0.55x light into House 3 F1 south window (8,9,1).
        let h3_lamp = block.local_to_world(6, 10, 0);
        let h3_f1_win = block.local_to_world(8, 9, 1);
        let f1_win_before = block.effective_light_u8(h3_f1_win);
        block.damage_tile(h3_lamp, DamageKind::Bullet);
        let h3_lamp_break = block.damage_tile(h3_lamp, DamageKind::Bullet).unwrap();
        assert!(h3_lamp_break.destroyed && h3_lamp_break.light_changed);
        let f1_win_after = block.effective_light_u8(h3_f1_win);
        assert!(
            f1_win_after < f1_win_before,
            "expected F1 window bleed light to drop ({f1_win_before} -> {f1_win_after})"
        );

        // Check full durability table values for props & interior walls.
        assert_eq!(
            tile_spec(TileKind::Prop(PropKind::Mailbox), Material::None).1,
            25
        );
        assert_eq!(
            tile_spec(TileKind::Prop(PropKind::Fence), Material::None).1,
            40
        );
        assert_eq!(
            tile_spec(TileKind::Prop(PropKind::Crate), Material::None).1,
            50
        );
        assert_eq!(
            tile_spec(TileKind::Prop(PropKind::Sofa), Material::None).1,
            75
        );
        assert_eq!(
            tile_spec(TileKind::Prop(PropKind::Shed), Material::None).1,
            80
        );
        assert_eq!(
            tile_spec(TileKind::Prop(PropKind::BirchTree), Material::None).1,
            120
        );
        assert_eq!(
            tile_spec(TileKind::InteriorWall, Material::None),
            (Material::Drywall, 1000)
        );
        assert_eq!(damage_against(Material::Drywall, DamageKind::Axe), 2);
        assert_eq!(
            damage_against(Material::Drywall, DamageKind::RocketBlast),
            1200
        );
        assert_eq!(
            damage_against(Material::Glass, DamageKind::ShotgunPellet),
            15
        );
        assert_eq!(
            damage_against(Material::WoodDoor, DamageKind::ShotgunPellet),
            12
        );
        assert_eq!(
            damage_against(Material::WoodSiding, DamageKind::RocketSplash),
            380
        );
        assert_eq!(
            damage_against(Material::Glass, DamageKind::CausticBurst),
            25
        );
        assert_eq!(
            damage_against(Material::Glass, DamageKind::MonsterMelee),
            15
        );

        // 999 axe hits on an exterior wall leave 1 HP; 1000th breaches to Rubble!
        let ext_wall = block.local_to_world(6, 23, 0);
        assert_eq!(block.tile_kind(ext_wall), TileKind::ExteriorWall);
        for _ in 0..999 {
            let hit = block.damage_tile(ext_wall, DamageKind::Axe).unwrap();
            assert!(!hit.destroyed);
        }
        assert_eq!(block.tile_render_info(ext_wall.pos(), 0).unwrap().hp, 1);
        let breach = block.damage_tile(ext_wall, DamageKind::Axe).unwrap();
        assert!(breach.destroyed);
        assert_eq!(breach.new_kind, TileKind::Rubble);
        assert!((breach.sound_radius - 14.0).abs() < 0.01);
    }

    #[test]
    fn rocket_breach_and_structural_overhang_collapse() {
        let (_, mut block) = block_for_seed(42);

        // 1 rocket blast breaches House 1 NW corner pillar (5,16) and wall (6,16) -> F1 collapse!
        let wall_a = block.local_to_world(6, 16, 0);
        let pillar = block.local_to_world(5, 16, 0);

        let r1 = block.damage_tile(wall_a, DamageKind::RocketBlast).unwrap();
        assert!(r1.destroyed);
        assert_eq!(r1.new_kind, TileKind::Rubble);

        let r2 = block.damage_tile(pillar, DamageKind::RocketBlast).unwrap();
        assert!(r2.destroyed);
        assert!(
            r2.collapsed_cells > 0,
            "destroying contiguous F0 supports must collapse F1 overhang"
        );

        let collapsed = block.take_collapsed_tiles();
        assert!(!collapsed.is_empty());
        assert!(block.take_collapsed_tiles().is_empty());

        for tile_f0 in &collapsed {
            assert_eq!(tile_f0.floor, 0);
            let tile_f1 = Tile3D::new(tile_f0.x, tile_f0.y, 1);
            let f1_info = block.tile_render_info(tile_f1.pos(), 1).unwrap();
            assert_eq!(f1_info.surface, Surface::Air);
            assert_eq!(f1_info.kind, TileKind::Empty);
            assert!(!block.has_floor(tile_f1));
        }

        // Verify StairsDown can never collapse even if all adjacent F0 supports are demolished.
        let stairs_f1 = block.local_to_world(12, 21, 1);
        for (lx, ly) in [(13, 20), (13, 21), (13, 22), (13, 23), (12, 23)] {
            let t = block.local_to_world(lx, ly, 0);
            let _ = block.damage_tile(t, DamageKind::RocketBlast);
        }
        assert_eq!(block.tile_kind(stairs_f1), TileKind::StairsDown);
        assert!(block.has_floor(stairs_f1));
    }

    #[test]
    fn door_toggle_and_room_reveal_visibility() {
        let (_, mut block) = block_for_seed(42);
        let kitchen_door = block.local_to_world(9, 18, 0);
        let kitchen_tile = block.local_to_world(7, 17, 0);
        let viewer_hall = block.local_to_world(10, 18, 0).center();

        // From the Hallway with the Kitchen door closed, Kitchen stays Hidden.
        block.update_visibility(viewer_hall, 0);
        assert_eq!(
            block
                .tile_render_info(kitchen_tile.pos(), 0)
                .unwrap()
                .visibility,
            TileVisibility::Hidden
        );

        // Toggle door open -> bumps terrain_version and reveals the full Kitchen room!
        let v_before = block.terrain_version();
        assert_eq!(block.toggle_door(kitchen_door), Some(true));
        assert!(block.terrain_version() > v_before);
        block.update_visibility(viewer_hall, 0);
        assert_eq!(
            block
                .tile_render_info(kitchen_tile.pos(), 0)
                .unwrap()
                .visibility,
            TileVisibility::Visible
        );

        // Toggle door closed -> Kitchen downgrades to Explored, no longer Visible!
        assert_eq!(block.toggle_door(kitchen_door), Some(false));
        block.update_visibility(viewer_hall, 0);
        assert_eq!(
            block
                .tile_render_info(kitchen_tile.pos(), 0)
                .unwrap()
                .visibility,
            TileVisibility::Explored
        );

        // Peeking through Kitchen west window from outside reveals the room!
        let outside_window = block.local_to_world(4, 18, 0).center();
        block.update_visibility(outside_window, 0);
        assert_eq!(
            block
                .tile_render_info(kitchen_tile.pos(), 0)
                .unwrap()
                .visibility,
            TileVisibility::Visible
        );

        // Standing on F1 near an F1 window sights F0 exterior tiles below!
        let f1_viewer = block.local_to_world(7, 22, 1).center();
        let f0_front_yard = block.local_to_world(7, 25, 0);
        block.update_visibility(f1_viewer, 1);
        assert_eq!(
            block
                .tile_render_info(f0_front_yard.pos(), 0)
                .unwrap()
                .visibility,
            TileVisibility::Visible
        );
    }

    #[test]
    fn window_vault_flashes_caches_and_doorway_assist() {
        let (_, mut block) = block_for_seed(42);

        // Window vaulting works once shattered.
        let win = block.local_to_world(7, 23, 0);
        let outside = block.local_to_world(7, 24, 0);
        let inside = block.local_to_world(7, 22, 0);
        assert_eq!(block.window_vault_target(outside, win), None);
        block.damage_tile(win, DamageKind::Axe);
        assert_eq!(block.window_vault_target(outside, win), Some(inside));
        assert_eq!(block.window_vault_target(inside, win), Some(outside));

        // Dynamic flashes brighten nearby tiles and expire after TTL ticks.
        let sample = block.local_to_world(10, 21, 0);
        let base_light = block.effective_light_u8(sample);
        block.register_dynamic_flash(sample.center(), 0, 4.0, 120, 2);
        assert!(block.effective_light_u8(sample) > base_light);
        block.tick_flashes();
        assert!(block.effective_light_u8(sample) > base_light);
        block.tick_flashes();
        assert_eq!(block.effective_light_u8(sample), base_light);

        // Cache can be taken only once and reports nearby correctly.
        let c0 = block.caches()[0].pos;
        let views = block.cache_views(c0.center(), 0, 1.0);
        assert!(views[0].nearby);
        assert_eq!(block.take_cache(0).unwrap().weapon, Some(Weapon::Axe));
        assert_eq!(block.take_cache(0), None);
        assert!(!block.cache_views(c0.center(), 0, 1.0)[0].nearby);

        // Doorway assist gently nudges an off-center player into House 1's open front door (9,23).
        let door_pos = block.local_to_world(9, 23, 0).center();
        let mut pos = Vec2::new(door_pos.x + 0.30, door_pos.y + 0.85);
        for _ in 0..20 {
            block.move_body(&mut pos, 0, Vec2::new(0.0, -0.06), 0.23, false, true);
        }
        assert!(
            pos.y < door_pos.y,
            "expected doorway assist to guide player through open front door, got y={} (door y={})",
            pos.y,
            door_pos.y
        );
    }
}
