See ./living-buildings-plan.md and related docs for context.

---

## Research Summary: Why These Algorithms (and What to Avoid)

To get true-to-life buildings and organic towns that still generate deterministically in milliseconds on mobile—and stay simple enough for lightweight coding agents to implement without breaking—four algorithmic choices matter most:

* **Grid-Snapped Organic Block Accretion + Frontage OBB Subdivision (over Straight Skeletons / Voronoi):** Full straight-skeleton lot subdivision is $O(N^3 \log N)$ and produces awkward triangular slivers that clash with tile-based buildings. Instead, grow orthogonal road blocks outward along terrain/water contours using a priority queue of candidate block expansions (stopping at steep slopes or water), then subdivide blocks via **rule-constrained OBB slicing** enforcing minimum area, aspect ratio, and road frontage. Every resulting lot stays axis-aligned to its street edge, guarantees road access, and drops cleanly onto the tile grid.
* **Wing-Massing + Topology-First Constrained BSP (over pure WFC or raw BSP):** Pure Wave Function Collapse struggles with global architectural invariants (e.g., "every house has exactly one kitchen adjacent to the living room and one front porch"). Instead, separate generation into three deterministic stages: (1) **Massing** (compose 1–3 overlapping rectangles into an L/T/U/split-level footprint), (2) **Topology-First Partitioning** (allocate public/entry rooms on the street face, place a minimal spine/hallway only when rooms exceed direct adjacency, and carve private rooms via constrained BSP), and (3) **Doorway Stenciling** (stamp the Phase 1 `3×3` clearance invariant before any walls or props touch the grid).
* **Hierarchical Wall-Hugging & Anchor-Child Furnishing (over random scatter):** Real rooms are organized by wall affinity and functional clusters—a sofa faces a TV across a coffee table; a dining table spawns chairs around its perimeter; kitchen counters run continuously along an exterior wall with the sink under a window. Solving **perimeter wall runs first**, then **anchor-child clusters**, then **center/floor fills**, with a hard reservation mask around every doorway apron and 1-tile walking spine, guarantees playable navigation and believable interiors.
* **Data-Driven Archetypes + Pluggable Traits (over enum sprawl):** Expressing building variants, room adjacency rules, and furnishing palettes as declarative structs (embeddable via `include_str!` RON/TOML later, or static Rust registries in Phase 3) behind `CompoundPlanner` and `StructureGenerator` traits lets Phase 4 (Police/Hospital) and Phase 6 (Traveling Circus) plug in without modifying chunk streaming or `CountyStage`.

---

# `docs/phase3-implementation-plan.md`

## 1. Phase 3 Scope & Legacy Teardown

**Goal:** Replace the legacy county town stamp (`5×5` grid) and `midx`/`midy` 4-quadrant building generator with a unified, extensible world-generation pipeline producing:

1. **Organic towns & road networks:** Contour-hugging morphologies (riverfront split-bank, highway ribbon, bay/lakefront, crossroads), separated residential subdivisions with cul-de-sacs, and regional highways with roadside **Gas Stations**.
2. **Unified `ParcelPlan` & `BlueprintGrid`:** Multi-structure parcels + outdoor dressing layer, retiring `ArmySub` and the placeholder Army Base.
3. **5 Residential House sub-archetypes:** Craftsman Bungalow (full porch), L-Wing Suburban Ranch (attached garage + driveway), 2-Story Center-Hall Colonial, Stepped Split-Level, and Side-by-Side Duplex.
4. **Open-span Grocery Store:** Load-bearing `TileKind::Pillar` grid, entrance vestibule, checkout lanes, `HighBlocker` gondola aisles, perimeter coolers, public restrooms, and rear stockroom/loading dock.
5. **Flagged rectangular block placeholders:** Any prop not yet in `assets/living-buildings/converted/` renders via a unified `PlaceholderBlock` primitive with accurate collider bounds and a tracked `PlaceholderId` for later asset swaps.

### Legacy Code to Scrap / Replace

* `crates/zombiebox-core/src/world/sites.rs`: Remove `ArmySub`, Army Base site generation, and the legacy 4-quadrant (`midx`/`midy`) box builder and corner-only 1×1 prop scatter.
* `crates/zombiebox-core/src/world/parcels.rs`: Replace single-building-per-parcel structs with `ParcelPlan` (`0..N` `PlacedStructure`s + `OutdoorLayer`).
* `crates/zombiebox-core/src/world/geography.rs`: Replace the fixed `5×5` rectangular town stamp with the organic block accretion + highway/subdivision planner.

---

## 2. File & Module Outline

Keep each subagent focused on a small set of files. New modules go under `crates/zombiebox-core/src/world/`:

| File | Responsibility | Status |
| --- | --- | --- |
| `world/geography.rs` | Regional highway spline/polyline graph, town seed placement, morphology selection (`Riverfront`, `HighwayRibbon`, `BayHugging`, `Crossroads`), and separated subdivision anchors. | Refactor |
| `world/town_planner.rs` | **New.** Organic block accretion along terrain/water contours, cul-de-sac spurs, zoning assignment (`Downtown`, `CommercialStrip`, `ResidentialSubdivision`, `HighwayService`), and OBB lot subdivision. | New |
| `world/parcels.rs` | `ParcelPlan`, `PlacedStructure`, `OutdoorLayer`, `CompoundPlanner` trait, and parcel-to-chunk materialization bridge. | Refactor |
| `world/blueprint.rs` | **New.** `BlueprintGrid` (sparse multi-floor `0..8F` cell buffer), `RoomId`, `WingId`, `DoorwayIntent`, `3×3` doorway clearance reservation, and validation checks. | New |
| `world/massing.rs` | **New.** Non-rectangular footprint composer (`Rect`, `LWing`, `TWing`, `SteppedSplit`, `Duplex`) with per-cell roof style and per-wing cutaway IDs. | New |
| `world/floorplan.rs` | **New.** Topology-first room graph solver, constrained BSP subdivision, hallway/stairwell placement, window placement, and open-span `Pillar` grid solver. | New |
| `world/furnish.rs` | **New.** Wall-hugging, anchor-child cluster, and aisle-run furnishing solver consuming `RoomFurnishRule`s + doorway/walkway reservation masks. | New |
| `world/archetypes/mod.rs` | **New.** Declarative registries (`BuildingArchetypeDef`, `AreaArchetypeDef`, `RoomFurnishRule`) + `StructureGenerator` trait. | New |
| `world/archetypes/houses.rs` | **New.** Definitions for the 5 House sub-archetypes + residential room palettes (Kitchen, Living, Bedroom, Bathroom, Garage, Porch). | New |
| `world/archetypes/retail.rs` | **New.** Grocery Store + Highway Gas Station (canopy + convenience store) archetypes and aisle/checkout/pump planners. | New |
| `world/dressing.rs` | Update to consume `OutdoorLayer` (driveways, sidewalks, fences, yard trees, parking lot stripes, highway signage, gas station aprons). | Update |
| `action/prop_catalog.rs` | Add `PropVisual::PlaceholderBlock(PlaceholderBlockSpec)` entries for missing Phase 3 props, tagged with `#[placeholder]` / `asset_todo` IDs. | Update |
| `zombiebox-render3d/src/mesh/props.rs` | Render `PropVisual::PlaceholderBlock` as muted, beveled/tinted cuboids respecting `PropSpan`, `HighBlocker` height, and `DoorAxis`/facing rotation. | Update |

---

## 3. Core Data Structures & Extensibility Traits

Place these in `world/parcels.rs`, `world/blueprint.rs`, and `world/archetypes/mod.rs` **first** (WP 3.1) so all downstream subagents compile against a frozen contract.

```rust
// --- world/parcels.rs ---

#[derive(Clone, Debug)]
pub struct ParcelPlan {
    pub parcel_id: u64,              // Deterministic 64-bit UID (Phase 1 convention)
    pub bounds: GridRect,            // World-tile AABB
    pub street_edge: CardinalDir,    // Primary road frontage
    pub zone: ZoneKind,              // Residential, Commercial, HighwayService, Civic, Compound
    pub structures: Vec<PlacedStructure>,
    pub outdoor: OutdoorLayer,
}

#[derive(Clone, Debug)]
pub struct PlacedStructure {
    pub structure_id: u64,
    pub archetype_id: &'static str,  // e.g. "house.craftsman_bungalow", "retail.grocery"
    pub origin: (i32, i32),          // World tile origin
    pub pad_elevation: f32,          // Flattened terrain pad height
    pub facing: CardinalDir,         // Rotates local blueprint -> world
    pub blueprint: BlueprintGrid,
}

#[derive(Clone, Debug, Default)]
pub struct OutdoorLayer {
    pub ground_overrides: Vec<(i32, i32, GroundSurface)>, // Asphalt, Concrete, Driveway, PorchWood
    pub fences: Vec<FenceSegment>,
    pub props: Vec<PlacedProp>,       // Mailboxes, shopping cart corrals, fuel pumps, dumpsters
    pub signs: Vec<PlacedSign>,       // Exterior fascia, road signs, gas price pylons
    pub vehicle_slots: Vec<VehicleSlot>, // Driveways, parking spaces, loading docks
}

pub trait CompoundPlanner: Send + Sync {
    fn id(&self) -> &'static str;
    fn plan_parcel(&self, ctx: &ParcelContext, rng: &mut StdRng) -> ParcelPlan;
}

pub trait StructureGenerator: Send + Sync {
    fn archetype_id(&self) -> &'static str;
    fn generate(&self, lot: GridRect, facing: CardinalDir, rng: &mut StdRng) -> BlueprintGrid;
}

```

```rust
// --- world/blueprint.rs ---

pub type WingId = u8;
pub type RoomId = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomKind {
    Porch, Foyer, LivingRoom, Kitchen, Dining, Bedroom, Bathroom,
    Hallway, Garage, Stairwell,
    RetailVestibule, CheckoutZone, SalesFloor, Stockroom, LoadingDock,
    PublicRestroom, ConvenienceStore, FuelCanopy,
}

#[derive(Clone, Debug)]
pub struct BlueprintCell {
    pub floor: u8,                     // 0..MAX_COUNTY_FLOORS (8)
    pub wing_id: WingId,               // Drives per-wing cutaway masks (Phase 1)
    pub room_id: RoomId,
    pub room_kind: RoomKind,
    pub tile: TileKind,                // Floor, Wall, Pillar, StairsUp, StairsDown, Doorway
    pub roof_style: Option<RoofStyle>, // Per-cell roof emission for stepped/split-level wings
    pub walls: EdgeMask,               // N/S/E/W wall/window/door edge specs
    pub reserved: ReserveFlags,        // DOOR_APRON | WALK_SPINE | WINDOW_LIGHT | OCCUPIED
}

#[derive(Clone, Debug)]
pub struct BlueprintGrid {
    pub width: u16,
    pub height: u16,
    pub max_floor: u8,
    pub cells: HashMap<(u16, u16, u8), BlueprintCell>, // Sparse storage matching Phase 1
    pub rooms: Vec<RoomMeta>,          // Bounds, RoomKind, WingId, connected_rooms
    pub doors: Vec<DoorIntent>,        // Pos, floor, DoorAxis, DoorKind, exterior flag
    pub props: Vec<PlacedProp>,        // Multi-tile PropAnchor + PropSpan + attachments
    pub signs: Vec<PlacedSign>,        // Uses Phase 2 signage.rs + Readables
}

```

```rust
// --- action/prop_catalog.rs (Placeholder support) ---

#[derive(Clone, Debug)]
pub struct PlaceholderBlockSpec {
    pub todo_id: &'static str,   // e.g. "ASSET_TODO_GONDOLA_SHELF"
    pub size_m: [f32; 3],        // Width (X), Depth (Y), Height (Z) in meters
    pub color_rgba: [f32; 4],    // Distinct muted debug/blockout tint
    pub high_blocker: bool,      // True for 1.45m+ retail shelves / tall coolers
    pub label: &'static str,     // Human-readable name for gallery/inspector
}

```

```rust
// --- world/archetypes/mod.rs ---

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallPreference {
    AnyWall,
    ExteriorWall,
    InteriorWall,
    UnderWindow,        // Prefer edge with a window (e.g. kitchen sink)
    AvoidWindow,        // Forbid blocking a window (e.g. fridge, bookcase, wardrobe)
    OppositeOf(&'static str), // Face across the room from an already-placed prop_id (e.g. TV opposite sofa)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementStrategy {
    /// Pass 1: Hug a wall segment, facing inward into the room.
    WallHug {
        pref: WallPreference,
        /// Optional: repeat continuously along the wall (e.g. kitchen counters, perimeter freezers).
        run_length: Option<(u8, u8)>, // (min_tiles, max_tiles)
    },
    /// Pass 1b (Retail): Parallel interior runs aligned to pillar/room axis with fixed aisle gaps.
    AisleGrid {
        aisle_width: u8,     // Minimum clear walking tiles between runs (e.g. 2)
        cross_aisle_margin: u8, // Clear tiles at ends of runs (e.g. 2..3)
        endcap_prop: Option<&'static str>, // e.g. Some("ASSET_TODO_GONDOLA_ENDCAP")
    },
    /// Pass 2: Spawn relative to a parent prop's local coordinate frame (rotated by parent facing).
    AnchorChild {
        parent_prop: &'static str,
        /// Local (right, forward) tile offsets relative to parent anchor.
        offsets: &'static [(i8, i8, RelativeFacing)],
    },
    /// Pass 3: Free-standing placement in open floor space (respects WALK_SPINE & DOOR_APRON).
    FloorOpen {
        prefer_corner: bool,
        min_wall_clearance: u8,
    },
    /// Layered attachment on top of an existing prop or wall (Phase 2 CellAttachment).
    Attachment {
        layer: AttachmentLayer, // CounterTop, WallMount, Ceiling
        host_prop: Option<&'static str>, // None = any valid wall/ceiling
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativeFacing {
    SameAsParent,
    TowardParent,     // e.g. dining chairs facing table, desk chair facing desk
    OppositeParent,   // e.g. coffee table or TV facing sofa
    Perpendicular,
}

#[derive(Clone, Copy, Debug)]
pub struct PropPlacementRule {
    pub prop_id: &'static str,     // Existing converted mesh id OR "ASSET_TODO_*"
    pub strategy: PlacementStrategy,
    pub count: (u8, u8),           // (min_required, max_allowed)
    pub chance: f32,               // 0.0..=1.0 (evaluated per extra instance after min_required)
    pub priority: u8,              // Higher runs first (100 = hero fixture, 50 = cluster, 10 = clutter)
    pub loot_table: Option<&'static str>, // e.g. Some("loot.kitchen_fridge")
    pub readable_pool: Option<&'static str>, // Optional Phase 2 Read lore tag
}

#[derive(Clone, Copy, Debug)]
pub struct RoomFurnishRule {
    pub room_kind: RoomKind,
    pub min_walk_spine_width: u8,  // 1 for houses, 2 for retail/public
    pub rules: &'static [PropPlacementRule],
}
```

---

## 4. Key Algorithms

### Algorithm 1: Organic Town Accretion & Highway Network (`world/geography.rs`, `world/town_planner.rs`)

1. **Regional Spine (Highways):** Connect map edges and major town seeds with gently curved, terrain-aware polylines (A* on a coarse 16-tile grid penalizing steep slopes and water crossings, keeping bridge crossings short and perpendicular).
2. **Town Morphology Selection:** Pick morphology from local geography around each town seed:
* *Riverfront Split-Bank:* Main street parallel to river + 1–2 bridges connecting commercial bank and residential bank.
* *Highway Ribbon:* Commercial strip + motels/gas stations hugging the highway, with residential streets branching perpendicularly behind.
* *Bay/Lakefront:* Crescent waterfront road with civic/commercial core and hillside residential tiers.
* *Crossroads:* Classic 2-arterial intersection with downtown core tapering into subdivisions.


3. **Contour-Hugging Block Accretion:**
* Seed 1–2 core commercial blocks at the town anchor.
* Maintain a priority queue of candidate adjacent blocks (scored by flatness, distance to core, and alignment to river/highway tangent).
* Pop and place blocks until the town's target block budget is reached, rejecting blocks that overlap water or exceed slope threshold.
* **Separated Subdivisions:** Spawn 1–3 detached residential clusters 12–24 tiles off secondary roads, connected by a single collector road ending in a **cul-de-sac loop/bulb** (`6×6` turnaround pad).


4. **Rule-Constrained OBB Lot Subdivision:**
* For each block, recursively split along its longest axis (with small seeded jitter `±15%`) until child lots satisfy zone constraints:
* *Residential:* width `12..18`, depth `14..22`, **must** retain ≥8 tiles of road frontage.
* *Grocery/Retail:* reserve an entire block or half-block (`28..40` × `24..34`) with front parking lot + rear alley/loading access.
* *Highway Gas Station:* snap directly to highway frontage (`22×18` lot) at town outskirts and rural highway junctions.





### Algorithm 2: Wing Massing & Topology-First Floorplan (`world/massing.rs`, `world/floorplan.rs`)

1. **Massing Composer:** Build the footprint from 1–3 axis-aligned rectangles tagged with `WingId` and `floor_count`:
* *Craftsman Bungalow:* `1F` main rect + `WingId(1)` recessed front porch strip.
* *L-Wing Suburban Ranch:* `1F` main living wing + protruding `WingId(1)` garage wing facing the driveway.
* *Center-Hall Colonial:* `2F` symmetric box with central `1F`/`2F` foyer/hallway and staircase.
* *Stepped Split-Level:* `WingId(0)` (`1F` living/kitchen) + `WingId(1)` (`2F` bedroom stack with per-cell gable roof on both heights).
* *Side-by-Side Duplex:* Mirrored twin units sharing a solid party wall (`WingId(0)` and `WingId(1)`).


2. **Room Allocation (Graph-First Constrained BSP):**
* Stamp fixed anchor rooms first: **Porch/Foyer** on street face, **Garage** on driveway wing, **Stairwell** in multi-story cores (aligned across `F0` and `F1`).
* Partition remaining interior cells by slicing along the longest axis, assigning rooms from the archetype's weighted `RoomSpec` list (`LivingRoom` adjacent to `Foyer`, `Kitchen` adjacent to `LivingRoom`/exterior wall, `Bedroom`s and `Bathroom` off hallway or private zone).
* If any room lacks a path to `Foyer` without passing through a `Bedroom` or `Bathroom`, carve a 2-tile-wide **Hallway** along the shared interior spine.


3. **Mandatory `3×3` Doorway Stencil (Phase 1 Invariant):**
* For every interior and exterior door in `doors`: record explicit `DoorAxis` (`NorthSouth` or `EastWest`), mark the 2 jamb tiles as solid wall, and flag the 2×3 front/back apron cells with `ReserveFlags::DOOR_APRON`.
* Reject or shift any door placement whose apron intersects a perpendicular wall, stairwell, or another door's jamb—guaranteeing the 64-seed `0.23m` swept-capsule test passes out of the box.



### Algorithm 3: Open-Span Retail & Column Grid (`world/archetypes/retail.rs`)

For the **Grocery Store** (and future warehouses/civic halls):

1. Do **not** run residential BSP on the sales floor. Instead, partition the building into 4 macro-zones:
* Front strip (`depth 4`): **Entrance Vestibule** + **Checkout Lanes** (parallel conveyor/till rows spaced 3 tiles apart) + **Shopping Cart bay**.
* Center span (`65%` of area): **Sales Floor** with a regular `TileKind::Pillar` structural grid (every 6×6 tiles) so Phase 1/2 structural overhang collapse still works if pillars are destroyed.
* Rear strip (`depth 6`): **Stockroom**, **Loading Dock** (exterior roll-up bay + crate stacks), and **Public Restrooms** (with Male/Female/WC relief plaques via `signage.rs`).


2. Generate parallel **Gondola Aisle Runs** (`HighBlocker` 1.45m shelves) aligned with the pillar grid, guaranteeing 3-tile-wide main cross-aisles and 2-tile-wide shelf aisles with endcap displays and overhead aisle number signs.

### Algorithm 4: Hierarchical Wall-Hugging Furnishing Solver (`world/furnish.rs`)

Run per room after all walls, windows, and `DOOR_APRON`s are finalized:

1. **Reserve Navigation Spine:** Compute a shortest path (BFS) connecting all doors in the room; mark those cells `ReserveFlags::WALK_SPINE` (width 1–2 tiles).
2. **Pass 1 — Wall-Hugging Fixtures:** Place wall-anchored props (`kitchen_cabinet`, `kitchen_stove`, `kitchen_fridge`, `bed_double`, `bathtub`, `toilet`, `bookcase`, `desk`, `television`) facing inward along unreserved wall segments. Prefer `kitchen_sink` under a window edge; forbid tall props (`bookcase`, `fridge`) from blocking windows.
3. **Pass 2 — Anchor-Child Clusters:** When a parent prop spawns, evaluate its child offsets (`table` → 2–4 `chair`s tucked around open sides; `desk` → `chair_desk` in front; `lounge_sofa` → `table_coffee` 1 tile forward facing `television`; `kitchen_cabinet` → `CounterTop` clutter).
4. **Pass 3 — Floor & Outdoor Fill:** Place free-standing props (`cardboard_box`, `crate`, `shopping_cart`, `first_aid_kit`) in remaining open corners and outdoor slots (driveway `car`, porch chairs, loading dock crates).
5. **Contextual Loot & Readables:** Bind container props (`fridge`, `desk`, `bookcase`, `cardboard_box`, `checkout_till`) to room-appropriate loot tables and seed 1–2 readable notes/store flyers/signs per structure using Phase 2's `Read` transport.

### Room Furnishing Rule Tables (WP 3.3–3.5 Reference)

#### Solver Execution Contract (`world/furnish.rs` — WP 3.3)

To keep lightweight subagents on rails, `furnish_room()` executes rules strictly in **descending `priority` order** with these hard invariants:

1. **Before any prop is placed:** Run BFS between all doors in `RoomMeta`; stamp `ReserveFlags::WALK_SPINE` (dilated to `min_walk_spine_width`). Never overwrite `DOOR_APRON` or `WALK_SPINE`.
2. **Multi-tile check:** Query `PropSpan` from `prop_catalog.rs`, rotate by candidate facing, and verify every footprint cell is inside `room_id`, unoccupied, and not reserved.
3. **Graceful fallback:** If `min_required` cannot fit (e.g. tiny bathroom), log a non-fatal placement miss in debug telemetry and continue—never panic or violate `DOOR_APRON`.
4. **Containers & Readables:** Any prop with `loot_table: Some(..)` spawns in its `closed` mesh variant (e.g. `kitchen-fridge-closed`, `desk-closed`) so Phase 2's container-open swap works automatically.

#### Residential House Rules (`world/archetypes/houses.rs` — WP 3.4)

| `RoomKind` | Priority | `prop_id` | Strategy | Count | Loot / Notes |
| --- | --- | --- | --- | --- | --- |
| **Kitchen** | 100 | `kitchen-sink` | `WallHug { UnderWindow }` | `1..1` | Falls back to `ExteriorWall` if no window |
|  | 95 | `kitchen-stove-electric` | `WallHug { ExteriorWall }` | `1..1` | Adjacent to sink/counter run |
|  | 90 | `kitchen-fridge-closed` | `WallHug { AvoidWindow }` | `1..1` | `loot.kitchen_food` |
|  | 80 | `kitchen-cabinet` | `WallHug { AnyWall, run: 2..5 }` | `2..5` | Fills gaps along counter wall; `loot.kitchen_pantry` |
|  | 60 | `table` | `FloorOpen { corner: false, clear: 1 }` | `0..1` | Only if room ≥ 4×4 |
|  | 55 | `chair` | `AnchorChild { parent: "table", TowardParent }` | `2..4` | Offsets: `(±1, 0)`, `(0, ±1)` |
| **LivingRoom** | 100 | `lounge-sofa` | `WallHug { InteriorWall }` | `1..1` | Hero anchor |
|  | 90 | `television-vintage` | `WallHug { OppositeOf("lounge-sofa") }` | `1..1` | Faces sofa across room |
|  | 85 | `table-coffee` | `AnchorChild { parent: "lounge-sofa", SameAsParent }` | `1..1` | Offset: `(0, +1)` in front of sofa |
|  | 60 | `bookcase-closed-doors` | `WallHug { AvoidWindow }` | `0..2` | `loot.residential_misc`; `readable.house_notes` |
|  | 30 | `cardboard-box-closed` | `FloorOpen { corner: true, clear: 0 }` | `0..2` | `loot.residential_clutter` |
| **Bedroom** | 100 | `bed-double` / `bed-single` | `WallHug { AvoidWindow }` | `1..1` | `bed-double` if room ≥ 4×4, else `bed-single` |
|  | 85 | `ASSET_TODO_NIGHTSTAND` | `AnchorChild { parent: "bed-*", SameAsParent }` | `1..2` | Offsets: `(-1, 0)`, `(+1, 0)`; `loot.bedside` |
|  | 80 | `ASSET_TODO_WARDROBE` | `WallHug { AvoidWindow }` | `1..1` | `loot.clothing` |
|  | 60 | `desk-closed` | `WallHug { UnderWindow }` | `0..1` | `loot.desk_drawer`; `readable.journal` |
|  | 55 | `chair-desk` | `AnchorChild { parent: "desk-closed", TowardParent }` | `0..1` | Offset: `(0, +1)` |
| **Bathroom** | 100 | `bathtub` | `WallHug { AvoidWindow }` | `1..1` | Skip if room < 3×3 |
|  | 95 | `toilet` | `WallHug { InteriorWall }` | `1..1` | — |
|  | 90 | `bathroom-sink` | `WallHug { AnyWall }` | `1..1` | `loot.medicine_cabinet` |
|  | 40 | `first-aid-kit` | `Attachment { CounterTop, Some("bathroom-sink") }` | `0..1` | `chance: 0.35`, `loot.medical_basic` |
| **Garage** | 90 | `car` | `FloorOpen { corner: false, clear: 1 }` | `0..1` | `chance: 0.65`, aligned to garage door |
|  | 60 | `crate` | `WallHug { AnyWall }` | `1..4` | `loot.tools_hardware` |
|  | 40 | `cardboard-box-closed` | `WallHug { AnyWall }` | `1..3` | `loot.residential_clutter` |
| **Porch** | 80 | `ASSET_TODO_PORCH_RAIL` | `WallHug { ExteriorWall, run: 2..8 }` | `2..8` | Perimeter edges except door apron |
|  | 40 | `chair` | `FloorOpen { corner: true, clear: 0 }` | `0..2` | Faces street |

#### Retail & Highway Service Rules (`world/archetypes/retail.rs` — WP 3.5)

| `RoomKind` | Priority | `prop_id` | Strategy | Count | Loot / Notes |
| --- | --- | --- | --- | --- | --- |
| **CheckoutZone** | 100 | `assisted-checkout-till-closed` | `AisleGrid { aisle: 2, margin: 2, endcap: None }` | `2..5` | Parallel checkout lanes; `loot.register` |
|  | 60 | `shopping-cart` | `WallHug { ExteriorWall, run: 2..4 }` | `2..4` | Vestibule cart corral |
| **SalesFloor** | 100 | `ASSET_TODO_GLASS_FREEZER` | `WallHug { ExteriorWall, run: 3..8 }` | `4..12` | Perimeter coolers; `loot.grocery_cold` |
|  | 90 | `ASSET_TODO_GONDOLA_SHELF` | `AisleGrid { aisle: 2, margin: 3, endcap: Some("ASSET_TODO_GONDOLA_ENDCAP") }` | `6..24` | `HighBlocker` stealth cover; `loot.grocery_dry` |
|  | 30 | `shopping-cart` | `FloorOpen { corner: false, clear: 0 }` | `1..3` | Abandoned in cross-aisles |
| **Stockroom** | 95 | `ASSET_TODO_STOCKROOM_RACK` | `WallHug { AnyWall, run: 2..6 }` | `2..6` | `HighBlocker`; `loot.grocery_bulk` |
|  | 70 | `crate` | `FloorOpen { corner: true, clear: 0 }` | `3..8` | Stacked near loading bay; `loot.grocery_bulk` |
|  | 50 | `cardboard-box-closed` | `FloorOpen { corner: false, clear: 0 }` | `2..6` | `loot.grocery_dry` |
| **PublicRestroom** | 100 | `toilet` | `WallHug { InteriorWall }` | `1..2` | Exterior door gets `signage.rs` WC plaque |
|  | 90 | `bathroom-sink` | `WallHug { AnyWall }` | `1..2` | — |
| **ConvenienceStore** (Gas Station) | 100 | `ASSET_TODO_CONVENIENCE_COUNTER` | `WallHug { InteriorWall, run: 2..3 }` | `2..3` | Behind front window |
|  | 95 | `assisted-checkout-till-closed` | `Attachment { CounterTop, Some("ASSET_TODO_CONVENIENCE_COUNTER") }` | `1..1` | `loot.register` |
|  | 85 | `ASSET_TODO_GLASS_FREEZER` | `WallHug { AvoidWindow, run: 2..4 }` | `2..4` | Rear wall drink coolers; `loot.grocery_cold` |
|  | 75 | `ASSET_TODO_GONDOLA_SHELF` | `AisleGrid { aisle: 2, margin: 2, endcap: None }` | `2..4` | 2 short snack aisles; `loot.grocery_dry` |
| **FuelCanopy** (Outdoor) | 100 | `ASSET_TODO_FUEL_PUMP` | `AisleGrid { aisle: 4, margin: 2, endcap: None }` | `2..4` | 4-tile drive-through lanes under canopy |
|  | 40 | `car` | `FloorOpen { corner: false, clear: 1 }` | `0..2` | Parked beside pumps (`chance: 0.5`)

 |



---

## 5. Asset Audit & Flagged Rectangular Block Placeholders

Audit against the 36 converted meshes currently in `assets/living-buildings/converted/`:

### Ready to Use Now (Existing `.mesh` Assets)

* **Residential Houses:** `bed-double`, `bed-single`, `lounge-sofa`, `table-coffee`, `table`, `chair`, `chair-desk`, `desk-closed`/`desk-opened`, `bookcase-closed-doors`/`bookcase-open`, `television-vintage`, `kitchen-cabinet`, `kitchen-sink`, `kitchen-stove-electric`, `kitchen-fridge-closed`/`kitchen-fridge-opened`, `toilet`, `bathtub`, `bathroom-sink`, `cardboard-box-closed`/`cardboard-box-opened`, `crate`, `car`.


* **Grocery Store & Gas Station:** `assisted-checkout-till-closed`/`assisted-checkout-till-opened`, `shopping-cart`, `crate`, `cardboard-box-closed`/`opened`, `toilet`, `bathroom-sink`, `desk-closed`/`opened`, `car`, `police-car`, `first-aid-kit`.


* **Signage:** All exterior fascia (`"GROCERY"`, `"GAS & MART"`, aisle numbers, restroom plaques, highway exit signs) use Phase 2's procedural 3D `signage.rs`—zero new mesh files needed.

### Missing Assets — Implement as `PlaceholderBlockSpec` & Flag for Later

Each entry below gets a `PlaceholderBlockSpec` in `action/prop_catalog.rs` with a searchable `ASSET_TODO_*` constant:

| `todo_id` | Where Used | Box Size (W×D×H m) | `HighBlocker` | Notes |
| --- | --- | --- | --- | --- |
| `ASSET_TODO_GONDOLA_SHELF` | Grocery Store | `2.0 × 0.9 × 1.45` | **Yes** | Double-sided aisle shelving (tiles into long runs) |
| `ASSET_TODO_GONDOLA_ENDCAP` | Grocery Store | `1.0 × 0.9 × 1.45` | **Yes** | Capped end of aisle with lootable cache |
| `ASSET_TODO_GLASS_FREEZER` | Grocery / Gas Mart | `2.0 × 0.9 × 2.00` | **Yes** | Perimeter cooler/freezer bank (emissive cool tint) |
| `ASSET_TODO_STOCKROOM_RACK` | Grocery Stockroom | `2.0 × 1.0 × 2.20` | **Yes** | Heavy warehouse pallet shelving |
| `ASSET_TODO_FUEL_PUMP` | Gas Station | `0.9 × 0.7 × 1.65` | **Yes** | Island gas dispenser under overhead canopy |
| `ASSET_TODO_CONVENIENCE_COUNTER` | Gas Station | `2.0 × 0.9 × 1.05` | No | Front clerk counter (pairs with checkout till) |
| `ASSET_TODO_WARDROBE` | House Bedroom | `1.0 × 0.6 × 1.85` | **Yes** | Lootable bedroom closet/dresser |
| `ASSET_TODO_NIGHTSTAND` | House Bedroom | `0.5 × 0.5 × 0.60` | No | Flanks `bed-double` / `bed-single` |
| `ASSET_TODO_PORCH_RAIL` | House Porch | `1.0 × 0.15 × 0.95` | No | Craftsman bungalow perimeter railing |
| `ASSET_TODO_DUMPSTER` | Rear Alleys / Retail | `2.0 × 1.2 × 1.35` | **Yes** | Commercial rear-lot cover + loot container |
| `ASSET_TODO_STREETLIGHT` | Towns / Highways | `0.3 × 0.3 × 4.50` | No | Street corner pole (emitter hook for night) |
| `ASSET_TODO_MAILBOX` | House Curbs | `0.3 × 0.4 × 1.15` | No | Street-edge residential detail |

---

## 6. Subagent Work Packages (Sequential / Parallel Handoffs)

Each package is sized for a single lightweight subagent session with explicit file ownership and automated `cargo test` acceptance gates.

### WP 3.1: Core Contracts, Blueprint Grid & Placeholder Renderer

* **Files:** `world/parcels.rs`, `world/blueprint.rs`, `world/archetypes/mod.rs`, `action/prop_catalog.rs`, `zombiebox-render3d/src/mesh/props.rs`.
* **Deliverables:** Define `ParcelPlan`, `PlacedStructure`, `OutdoorLayer`, `BlueprintGrid`, `CompoundPlanner`, `StructureGenerator`, and `PlaceholderBlockSpec`. Add the 12 `ASSET_TODO_*` entries to `prop_catalog.rs` and render them in `mesh/props.rs` as tinted boxes matching `size_m` and facing. Add a `#[cfg(test)]` check that iterates over every `RoomFurnishRule` in the registry and asserts every `prop_id` resolves in `prop_catalog.rs`.
* **Acceptance Gate:** `cargo test -p zombiebox-core` and `cargo test -p zombiebox-render3d` pass; a unit test verifies `BlueprintGrid` doorway `3×3` apron reservation rejects overlapping walls/props.

### WP 3.2: Massing Composer & Topology-First Floorplan Solver

* **Files:** `world/massing.rs`, `world/floorplan.rs`.
* **Deliverables:** Implement the 5 massing shapes (`Rect`, `LWing`, `TWing`, `SteppedSplit`, `Duplex`) with per-wing `WingId` and per-cell `RoofStyle`. Implement graph-first constrained BSP room allocation, stairwell alignment across floors, exterior windows, and the `3×3` doorway stencil.
* **Acceptance Gate:** Unit test across 64 seeds proves every generated building has: (a) 100% room connectivity from exterior entrance via BFS, (b) zero doorway apron violations, (c) aligned stairwells on 2F structures.

### WP 3.3: Hierarchical Furnishing Solver

* **Files:** `world/furnish.rs`.
* **Deliverables:** Implement the 3-pass furnishing solver (walk-spine BFS reservation → wall-hugging pass → anchor-child clusters → floor fill), respecting `PropSpan` footprints, window clearance, and `CounterTop`/`WallMount` attachment layers from Phase 2.
* **Acceptance Gate:** Unit test verifies zero props overlap walls, zero props intrude into `DOOR_APRON` or block the room `WALK_SPINE`, and anchor-child clusters (e.g., sofa + coffee table, table + chairs) orient toward each other.

### WP 3.4: 5 Residential House Archetypes

* **Files:** `world/archetypes/houses.rs`.
* **Deliverables:** Author `StructureGenerator` + `RoomFurnishRule` definitions for Craftsman Bungalow, L-Wing Suburban Ranch (with garage + driveway car slot), 2-Story Center-Hall Colonial, Stepped Split-Level, and Side-by-Side Duplex using existing Kenney meshes + `WARDROBE`/`NIGHTSTAND`/`PORCH_RAIL`/`MAILBOX` placeholders.
* **Acceptance Gate:** 64-seed generation test confirms all 5 sub-archetypes produce distinct footprints, distinct room counts, furnished kitchens/baths/bedrooms/living rooms, and pass the bilateral `0.23m` doorway capsule sweep.

### WP 3.5: Grocery Store & Highway Gas Station Archetypes

* **Files:** `world/archetypes/retail.rs`.
* **Deliverables:** Implement the open-span Grocery Store (vestibule, checkout lanes with `assisted-checkout-till`, `TileKind::Pillar` grid, `HighBlocker` gondola aisles, perimeter freezers, public restrooms with `signage.rs` plaques, rear stockroom/loading dock) and Highway Gas Station (overhead `FuelCanopy` + `ASSET_TODO_FUEL_PUMP` islands + convenience mart + highway pylon sign).
* **Acceptance Gate:** Unit test verifies `TileKind::Pillar` spacing, 2-tile aisle navigability from entrance to stockroom and restrooms, `HighBlocker` collider flags on gondola shelves, and readable exterior/aisle signage.

### WP 3.6: Organic Town Planner, Highways & Subdivisions

* **Files:** `world/geography.rs`, `world/town_planner.rs`, `world/dressing.rs`.
* **Deliverables:** Replace the `5×5` town stamp with terrain-aware highway polylines, morphology-driven contour-hugging block accretion, detached cul-de-sac subdivisions, and OBB lot subdivision. Wire `OutdoorLayer` dressing (driveways, sidewalks, parking lots, fences, dumpsters, streetlights).
* **Acceptance Gate:** Determinism test (same seed → identical `ParcelPlan` hashes); multi-seed geometry test confirms zero lots overlap water, 100% of lots touch a road edge, and every town spawns at least one Grocery Store, multiple House archetypes, and highway Gas Stations.

### WP 3.7: Legacy Teardown, `CountyStage` Wiring & Headless 3D Verification

* **Files:** `world/sites.rs`, `action/county.rs`, `crates/zombiebox-core/tests/`.
* **Deliverables:** Delete legacy `ArmySub` and `midx`/`midy` box generation from `world/sites.rs`. Wire `CountyStage` chunk materialization exclusively to `ParcelPlan` + `BlueprintGrid`. Update integration tests.
* **Acceptance Gate:**
1. `cargo test -p zombiebox-core --lib generated_doors_and_gates_pass_runtime_bilateral_capsule_sweep` passes across seeds `0..63`.
2. Full `cargo test --workspace` passes.
3. Headless `check3d` / `preview3d` renders a seed-42 town showing non-rectangular houses, grocery aisles, gas station canopy, and organic road layout without chunk-seam panics or cutaway artifacts.


