# Exterior/interior assets, room furnishing, signage, and infected ecology

Technical architecture accompanying [living-buildings-plan.md](living-buildings-plan.md). Preserves Zombiebox's **Sega Genesis / early PlayStation** aesthetic, zero-UV 48-byte `WorldVertex` pipeline (`world.wgsl`), and **60 Hz (`16.7 ms`) budget on modest Android handsets**.

---

## 1. Current asset, prop, and zombie limitations

- **Coarse unoriented `1×1` props:** `sites.rs` emits a single `Feature::Furniture` in room corners, mapped in `county.rs::prop_for_room` across only 10 `PropKind`s (`Table, Sofa, Counter, Bed, Crate, Fence, Mailbox, Bench, Shed, BirchTree`). `build_chunk_props_mesh` ignores orientation for everything except `Fence` and allocates fresh `MeshBuffer`s per tile with unculled bottom faces.
- **No vertical layers or sight-blocking shelves:** Every cell holds one `TileKind` and every furniture collider is `TileObstruction::LowProp` (`0.58–0.78m` tall, LOS-transparent), preventing grocery shelf aisles, tall lockers, wall-mounted TVs/signs, counter-top registers, or overhead surgical lamps.
- **Generic military crates everywhere:** `SupplyCache` sits on bare floor tiles and always renders as the same olive-green military box (`cache_mesh`), whether in a kitchen, pharmacy, or police armory.
- **Cuboid fallback for 3 of 4 zombie types:** In `animation.rs`, `zombie = m.kind == MonsterKind::Shambler` — so `Brisk`, `Bloater`, and `Shooter` fall back to scaled `0.5` unit cuboids! `Monster.variant` (`u8`) is already seeded in `county.rs` and exposed on `MonsterView`, but completely unused by the renderer.

---

## 2. Interior furnishing & exterior decoration system

### 2.1 Oriented, multi-tile & layered prop representation
Replace bare `PropKind` cells with `PropId(u16)` indexing a declarative `PROP_DEFS` registry, explicit cardinal orientation, multi-tile span links, and sparse sub-tile attachments:

- **Multi-tile floor occupancy (`O(1)` collision & raycast):**
  - `TileKind::PropAnchor { id: PropId, dir: CardinalDir, style_seed: u8 }` at the prop's origin `(x0, y0)`.
  - `TileKind::PropSpan { anchor_dx: i8, anchor_dy: i8 }` on covered cells of `2×1`, `2×2`, `3×2`, or `4×2` props (beds, sofas, surgical tables, checkout lanes, shelf gondolas, parked ambulances). Damaging any span cell forwards HP loss to the anchor; destruction turns the full footprint to `Rubble` and invalidates chunk meshes with halo.
- **Sparse attachment layers (`chunk.attachments: Vec<CellAttachment>`):**
  - Supports `PropLayer::{Floor, CounterTop, WallMount, Ceiling, Rooftop}` with zero byte overhead on empty tiles — allowing one tile to stack `Floor(Counter) + CounterTop(CashRegister) + Ceiling(AisleSign)` or `ExteriorWall + WallMount(StorefrontSign) + Rooftop(HvacUnit)`.

### 2.2 Tactical cover, materials, prop lighting & contextual loot containers
1. **`CoverHeight` tiers:**
   - `Passable` (`0–0.05m`): Floor rugs, entrance mats, hazard stripes, lobby seals.
   - `LowCover` (`0.45–0.90m`): Counters, beds, sofas, desks, gurneys, produce bins (blocks walking; shoot/see over).
   - `HighBlocker` (`1.35–1.95m`): Supermarket gondolas, lockers, fridges, wardrobes, tearable hospital curtains (`TileObstruction::HighProp` — blocks walking, bullets, and AI line-of-sight for stealth & flanking).
   - `Overhead` (`>= 1.85m` clearance): Ceiling surgical lights, hanging department signs, awnings.
2. **New destructible materials & prop lights:**
   - Add `Material::{MetalSheet, Ceramic, Electronics, Upholstery, Canvas}`.
   - Props with `PropLightSpec` register `LightKind::{TelevisionGlow, SurgicalLamp, ExitSign, FluorescentTube, NeonSign}`; smashing a glowing TV, neon sign, or surgical lamp disables its light source dynamically.
3. **Contextual instanced loot containers (zero chunk rebuild on loot):**
   - Bind `SupplyCache` to container props (`Fridge`, `PharmacyShelf`, `MedicineCabinet`, `CrashCart`, `GunLocker`, `OfficeDesk`, `Nightstand`, `ToolChest`, `MilitaryCrate`).
   - Exclude active lootable containers from static chunk meshes and draw them via **GPU-instanced `(ClosedPrototype, OpenedPrototype)` pairs** with a subtle brass/amber handle glint and specific HUD prompts (*"Search Pharmacy Shelf"*, *"Search Gun Locker"*, *"Search Fridge"*).

### 2.3 Constraint & anchor-based room furnishing solver (`world/furnish.rs`)
Runs deterministically in its own seed domain (`0x4655_524e_4953_4831`) after doors and stairs are finalized:
1. **Clearance reservation:** Reserve 1-tile doorway/stair aprons + Manhattan circulation spines connecting all room openings; tag walls as `SolidWall(dir)` vs `WindowWall(dir)` (**never place `HighBlocker` props over windows**, preserving vaulting and sightlines).
2. **Room grammars:**
   - **Kitchen:** L/galley `KitchenCounter` run along `SolidWall`/`WindowWall`, `KitchenSink` under window, `Stove` + `RangeHood`, lootable `Fridge`, and `DiningTable` + `Chair`s on open floor.
   - **Living Room:** `TvStand` + glowing `Television` on longest `SolidWall`, `CoffeeTable` + `Sofa` (`2×1` oriented *toward* the TV) over a `FloorRug`, corner `FloorLamp` / `Bookshelf`.
   - **Bedroom & Bathroom:** `Bed` (`2×1`/`2×2`, headboard against `SolidWall`) + lootable `Nightstand`s + `Wardrobe` + window `Desk`; Bathrooms get ceramic tile, `Bathtub`/`Shower`, `Toilet`, `PedestalSink` + `WallMirror` + lootable `MedicineCabinet`.
   - **Grocery Store:** Parallel `CheckoutLane` (`ConveyorCounter` + `CashRegister`) pods near exit; `2`-tile aisles between `HighBlocker` `GondolaShelf` runs with `EndcapShelf` caches; `ProduceBin` islands; `GlassFreezer` perimeter; rear `PalletRack`s.
   - **Police Station:** Ballistic-glass `ReceptionCounter`, `2×2` `DeskPod`s (`MetalDesk` + `DesktopMonitor` + `SwivelChair` + `FilingCabinet`), `Interrogation` `MetalTable` + opposing chairs + `OneWayMirror`, cell `CellBunk` + `SteelToilet`, armory `GunLocker`s + `WeaponRack`s.
   - **Hospital:** `NurseStation` counters, linked `WaitingChairs`, `Gurney`s/`Wheelchair`s, ward `HospitalBed` + `IvPole` + `VitalMonitor` + `PrivacyCurtain`, OR centered `SurgicalTable` + overhead `SurgicalLamp` + `AnesthesiaMachine` + `ScrubSink`, flush `ElevatorDoors` + `CallButtonPanel`.

---

## 3. Diegetic retro signage & `Read` environmental text system

Because `WorldVertex` (48 bytes) intentionally carries **no UVs** and `world.wgsl` samples zero color textures, wayfinding stays **100% diegetic in the 3D world** (no automatic floating zone banners) paired with an explicit **`Read` contextual action** for signs, notes, and journals:

1. **Procedural 3D Relief Glyph & Pictogram Mesher (`render3d/src/mesh/signage.rs`):**
   - Compact `< 512 B` `GLYPH_5X7` bitmask font (`A–Z`, `0–9`, directional arrows, `+` cross) plus `8×8` relief pictograms (Male/Female/WC restroom icons, Police shield, Pharmacy cross, Stairs/Elevator icons).
   - **Greedy 2D rectangle merging** packs each character into `2–5` boxes (`4–10` front+side triangles with `FaceMask` back-face stripping), so `"POLICE"`, `"COUNTY HOSPITAL"`, `"SUPERMARKET"`, `"<- SURGERY 5F"`, or `"EXIT"` plaques cost only **50–110 triangles** baked directly into the chunk mesh (with optional `MaterialFlags::UNLIT` neon/backlit glow and `CUTAWAY_EXEMPT` on ground-floor entrance canopies).
   - **Roadside highway signposts** at arterial intersections point toward `HOSPITAL`, `DOWNTOWN`, and `POLICE`.
   - **Cutaway-proof floor inlays** (`RedCrossMat`, `PoliceSeal`, `YellowHazardStripe`) keep room roles readable even when front walls dissolve in Isometric view.
2. **Contextual `Read` Action (`InteractPrompt::Read`) for Signs, Notes & Journals:**
   - Attach optional `ReadableTextId` metadata to both **signage props** (`HighwaySignpost`, `HospitalDirectory`, `DepartmentSign`, `BulletinBoard`) and **environmental lore props** (`SurvivorJournal` on nightstands/tables, `PatientChart` at ward beds, `IncidentLog` on police desks, `PinnedNote` near locked armory/pharmacy doors).
   - Standing in front of a readable object surfaces **`InteractPrompt::Read`** (*"Read Directory"*, *"Read Journal"*, *"Read Sign"*). Triggering it opens a lightweight retro text card displaying the seeded text — making zoomed-out signs effortless to inspect on demand while unlocking environmental storytelling and organic gameplay clues (e.g., *"Sgt. Miller took the Armory keycard down to Interrogation"*).

---

## 4. Asset creation pipeline & 60 Hz mobile budgets

1. **Tier 1 — Parametric Rust Furniture Kit (`render3d/src/mesh/kit.rs`):** Composable box/bevel/slat/tube helpers for ~70% of rectilinear props, stripping unseen `NEG_Y` bottom and wall-mounted back faces (**~30% triangle reduction**).
2. **Tier 2 — Style-Palette Recipes:** Props reference semantic slots (`WoodPrimary`, `MetalTrim`, `FabricMain`, `ClinicalSurface`, `Emissive`) resolved per `BuildingStyle` + `style_seed`, turning 1 mesh into residential, office, hospital, or grunge variants.
3. **Tier 3 — Indexed Python-to-Rust/GLB Generators (`scripts/generate_*_mesh.py`):** For organic/curved hero meshes (zombie rigs/attachments, bloaters, porcelain fixtures, cruisers/ambulances, Ferris wheel gondolas). Emits **indexed `(POINTS, FACES)` tables** like `motorcycle.rs` instead of unindexed triangle literals (**~65% smaller source & faster compiles**).

| Render path | Batching strategy | Mobile target (modest Android @ 60 Hz) |
| --- | --- | --- |
| Static props, signs, facades | Cached `OnceLock<PropMeshLibrary>` copied via `Mat4` into **1 static VB/IB per `16×16` chunk** | `< 0.35 ms` CPU rebuild per dirty chunk; `< 20k tris/chunk`; **15–25 chunk draws/frame** |
| Lootable furniture containers | **GPU Instanced** `(Closed, Opened)` prototypes in `DynamicInstanceBuffer` | **0 chunk rebuilds on search**; ~6–10 active batches/frame |
| Animated setpieces (Ferris wheel, fans) | **GPU Instanced** rigid parts driven deterministically from `sim_tick` | 2–6 batches when in view |
| Point lights (`MAX_POINT_LIGHTS = 32`) | Pre-filtered against `LocalVisibilityGrid != Hidden` before upload | Shrouded interior lights never steal active light slots |

---

## 5. Themed & mutated zombie variants per building

### 5.1 Decouple combat behavior (`MonsterKind`) from attire (`ZombieOutfit`)
- **Combat/AI Archetypes (`MonsterKind`):**
  - Existing: `Shambler`, `Brisk`, `Bloater`, `Shooter` (upgraded to hold/aim actual `Pistol`/`Shotgun` in police/military zones).
  - Extensible additions:
    - **`Armored`** (Riot Police / Heavy Soldier): Reduces frontal gunfire damage by ~65%; vulnerable to melee axe, knockback flanking, and explosives.
    - **`Screamer`** (Hospital Orderly / Alarmed Patient): Fragile; windup shriek emits an `r = 16` `NoiseEvent` pulling nearby rooms/floors.
    - **`Crawler`** (Hospital Trauma / Amputee): `0.42m` tall; moves unseen behind `LowCover` beds/counters and inflicts a brief ankle-bite slow.
    - **`Spitter`** (Hospital Lab / ICU Mutant): Replaces firearm `Shooter` inside hospitals with an organic arcing caustic glob leaving a short acid floor hazard.
- **Visual Outfits (`ZombieOutfit`):** `Civilian`, `PoliceOfficer`, `SwatRiot`, `HospitalPatient`, `NurseScrubs`, `Surgeon`, `HazmatLab`, `GroceryClerk`, `ArmySoldier`, `RuralWoods`, `CircusClown`.

### 5.2 Single-batch uniform dyeing (`TINT_EXEMPT`) + `ZombieSocket` attachments
- **Distinct base rigs (`generate_zombie_mesh.py`):** Author proper 6-part meshes for clothed humanoids (`Shambler`/`Brisk`/`Shooter`/`Screamer`/`Crawler` with archetype-specific gait/aim/prone poses), swollen caustic-sac **`Bloater`** (satisfying the `product-decisions.md` distinct-model milestone), and bulky **`Armored`** torso/head overrides.
- **Zero-batch-explosion uniform colors:** Claim 1 free bit in `MaterialFlags` (`TINT_EXEMPT = 1 << 4`) + 2 lines in `world.wgsl`. Mark zombie flesh, blood, eyes, and boots `TINT_EXEMPT`, and author clothing in shaded neutral grayscale. `InstanceRaw.color_tint` then dyes uniforms into **Police Navy, Nurse Teal, Surgical Green, Patient Gown Blue, Supermarket Red, Olive Drab, or Clown Stripes** inside the **exact same 6 GPU instance batches**!
- **Rigid `ZombieSocket` attachments (`25–70 tris` each):** Shared instanced `ActorPart`s at `HeadTop`, `Face`, `Chest`, and `HandR` (`PoliceCap`, `RiotHelmet`, `SurgicalCapMask`, `HazmatHood`, `TacticalVest`, `GroceryApron`, `MutantTumorCluster`, `ClownWig`, plus held `Pistol`/`Shotgun`), persisted onto `Corpse` on death.

### 5.3 Room-driven ecology summary
- **Police Station:** `PoliceOfficer` `Shooter`s & `Shambler`s in lobby/bullpen; orange/gray detainee `Brisk` runners in `Cells`; `SwatRiot` `Armored` + `Shooter`s guarding `Armory` & `Garage`.
- **Hospital:** `HospitalPatient` `Shambler`s & `Crawler`s + `NurseScrubs` in `Wards`; `Surgeon` `Brisk`s & `Screamer`s in `OperatingRoom`s; `HazmatLab` / tumor-mutated `Bloater`s & `Spitter`s in `Pharmacy`, `Labs`, and `ICU`.
- **Grocery Store:** `GroceryClerk` & `Civilian` `Shambler`s/`Brisk`s in shelf aisles; `Bloater`s in back stockroom/coolers.
- **Houses:** `Civilian` `Shambler`s & `Brisk`s contextualized by room density.
