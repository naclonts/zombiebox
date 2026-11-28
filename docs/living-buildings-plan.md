# Living buildings and extensible compounds plan

Turn Zombiebox's procedural buildings from uniform 1–3 story 4-quadrant boxes into recognizable, lived-in places with purposeful floorplans, rich interior/exterior props, readable signage, contextual infected variants, and an extensible compound architecture ready for future areas like fairgrounds or circuses.

- **Detailed algorithms & spatial architecture:** [building-generation-algorithms.md](building-generation-algorithms.md)
- **Props, furnishing grammars, signage & infected ecology:** [building-assets-and-ecology.md](building-assets-and-ecology.md)
- **Mesh sources and licenses:** [prop-asset-catalog.md](prop-asset-catalog.md)
- **Phase 2 task handoffs and acceptance checks:** [phase2-implementation-plan.md](phase2-implementation-plan.md)
- **Organic towns, freeways, gas stations & infinite world scaling:** [world-and-town-layout-plan.md](world-and-town-layout-plan.md)
- **Product checklist & milestones:** [product-decisions.md](product-decisions.md#planned-milestones-and-completion-checks) · [world-design.md](world-design.md)

---

## 1. Vision & core pillars

Today, `crates/zombiebox-core/src/world/sites.rs` builds almost every county structure from a single rectangle divided by a cross-hallway (`midx`, `midy`) into four boxes, capped at `COUNTY_FLOORS = 3`, furnished with one unoriented `1×1` prop in each room corner, lit only outdoors, and populated with generic military loot crates and cuboid-fallback non-shambler zombies.

This plan upgrades buildings across **four decoupled, data-driven pillars**:

| Pillar | What changes | Technical anchor |
| --- | --- | --- |
| **1. Building & compound generation** | Non-rectangular wing massing (L/T/U/courtyards, podium + stepped tower), architectural room graphs, keycard/breach security gating, open-span retail columns, 4–8 sparse floors, button-strip elevators, and multi-building parcels (retiring the placeholder Army Base until tanks/planes arrive). | [building-generation-algorithms.md](building-generation-algorithms.md) |
| **2. Exterior/interior assets, signage & `Read` lore** | Oriented multi-tile props (`1×1` to `4×2` + setpieces), layered attachments (`CounterTop`, `WallMount`, `Ceiling`, `Rooftop`), `HighBlocker` aisle cover, room furnishing solvers, contextual loot containers, diegetic 3D pixel-relief signage, and a `Read` action for signs, directories, notes, and journals. | [building-assets-and-ecology.md](building-assets-and-ecology.md#2-interior-furnishing--exterior-decoration-system) |
| **3. Building-specific infected ecology** | Separate `MonsterKind` AI/combat archetypes (`Shambler`, `Brisk`, `Bloater`, `Shooter`, `Armored`, `Screamer`, `Crawler`, `Spitter`) from `ZombieOutfit` uniforms (`PoliceOfficer`, `SwatRiot`, `NurseScrubs`, `Surgeon`, `HospitalPatient`, `GroceryClerk`, `CircusClown`) rendered in shared GPU instance batches. | [building-assets-and-ecology.md](building-assets-and-ecology.md#5-themed--mutated-zombie-variants-per-building) |
| **4. Extensibility & moddability** | Declarative `AreaArchetypeDef`, `BuildingArchetypeDef`, `PropDef`, `RoomFurnishRule`, and `EcologyProfileDef` registries plus pluggable `CompoundPlanner` / `StructureGenerator` traits — no hardcoded enum sprawl or `ArmySub` special cases. | [building-generation-algorithms.md](building-generation-algorithms.md#5-extensibility--moddability-new-buildings--whole-new-areas) |
| **5. Organic towns, highways & infinite world** | Replaces the `5×5` rectangular town stamp with contour-hugging town morphologies (riverfront split-bank, highway ribbon, bay-hugging), separated subdivisions, freeways with gas stations/motels, and a two-horizon path to an unbounded multi-region world. | [world-and-town-layout-plan.md](world-and-town-layout-plan.md) |

---

## 2. How signature building types come to life

### Police station (`2–3 floors`, fortified `T-Wing` or `L-Compound`)
- **Security gradient, keycards & breaching:** Exactly **one** unlocked public front entrance into a **Public Lobby** (with public restrooms and waiting benches), separated from staff areas by a ballistic-glass **Booking / Reception Desk** and locked security door. High-security rooms (**Booking Gate**, **Holding Cells**, **Armory**) require either **looted keys/keycards** (from officer zombies or desks) or **physical breaching** (reinforced steel armory doors resist light tools and require a keycard or heavy explosives). Rear fenced **Sallyport / Garage** and one steel rear fire exit; **zero ground-floor windows** in detention or armory bays.
- **Signature rooms & props:** Open detective **Office Bullpen** (`2×2` desk + monitor + swivel chair pods, readable incident logs), paired **Interrogation + Viewing Room** (one-way interior window, metal table, pendant lamp), **Holding Cells** (`MetalPole` barred walls/doors, bunk, steel toilet), and windowless reinforced **Armory** (lootable gun lockers and weapon racks).
- **Infected mix:** Uniformed `PoliceOfficer` shamblers/shooters (sidearms, chance to drop precinct keys/keycards), detainee brisk runners in cells, and `SwatRiot` (`Armored` front-ballistic-resistant) guards near the armory.

### Hospital (`4–6+ floors`, `2F` ER/diagnostic podium + stepped `4–6F` ward/surgical tower)
- **Vertical scale & button-strip elevators:** Sparse per-chunk floor layers lift `MAX_COUNTY_FLOORS` to **8** while *reducing* rural/suburban chunk RAM by ~60%. Central **2-car Elevator Bank** + dual **Fire Stairwells** connect all floors; stepping inside a powered elevator displays a **HUD floor button strip (`1F..NF`)** for direct floor selection, and arriving rings a bell on the target floor that draws nearby infected toward the doors.
- **Signature rooms & props:** Covered **Ambulance Bay** + **ER Trauma Bays** and **Main Lobby** on `1F`; locked **Pharmacy** (requires pharmacist/doctor keycard or breaching to reach dense medkit shelves) and **Clinical Labs** on `2F`; **Inpatient Wards** (`HospitalBed`s, IV poles, pulsing vital monitors, tearable privacy curtains, readable patient charts/journals, central 360° **Nurses' Station**) on `3F–4F`; **Operating Rooms** (`SurgicalTable` with guaranteed 360° clearance, overhead `SurgicalLamp` emitter, scrub sinks) and **ICU** on `5F+`.
- **Diegetic signage & `Read` action:** Illuminated exterior **`+` Red Cross** and `"COUNTY HOSPITAL"` / `"EMERGENCY"` signs; lobby directory, overhead corridor arrows (`"<- WARDS"`, `"SURGERY 5F"`, `"PHARMACY ->"`), floor numerals by stairs/elevators, and cutaway-proof floor inlays — all purely in-world, with a contextual **`Read` action** (`InteractPrompt::Read`) in front of directories, signs, notes, and journals.
- **Infected mix:** Gown-clad `HospitalPatient` shamblers and low-profile `Crawler`s in wards, fast `NurseScrubs` / `Surgeon` runners and `Screamer` orderlies in corridors/ORs, and `Bloater`s + caustic `Spitter` lab mutants in pathology/ICU.

### Grocery store / supermarket (`1–2 floors`, wide open-span retail box)
- **Open-span structure:** Replaces interior drywall partitions with a load-bearing `TileKind::Pillar` column grid so sightlines stay wide while preserving structural overhang collapse if columns are blasted.
- **Signature layout & props:** Entrance vestibule, parallel **Checkout Lanes** (conveyor counter + register + customer lane), **Gondola Shelf Aisles** (`HighBlocker` 1.45m tall shelves creating stealth corridors and destructible cover, capped with endcap caches and hanging aisle numbers), illuminated **Glass Freezers** along the perimeter, and a rear **Stockroom + Loading Dock** + public restroom.
- **Infected mix:** `GroceryClerk` (red apron) and `Civilian` shamblers/runners weaving through sight-blocking aisles, plus occasional `Bloater`s in cold storage.

### Residential houses (`1–2 floors`, 5 varied sub-archetypes)
- **Varied massing:** Seeded mix of **Craftsman Bungalows** (full porch), **L-Wing Suburban Ranches** (attached protruding garage on driveway), **2-Story Center-Hall Colonials**, **Stepped Split-Levels** (per-cell roof emission over 1F wings), and **Side-by-Side Duplexes**.
- **Signature rooms & props:** **Kitchens** (L/galley counters, sink under window, stove + range hood, lootable fridge, dining table), **Living Rooms** (sofa + coffee table oriented toward a glowing `Television` + stand over a rug), **Bedrooms** (`2×1`/`2×2` beds against solid walls, lootable nightstands, wardrobes), and **Bathrooms** (tub, toilet, sink + mirror/medicine cabinet).

### Universal public amenities & doorway clearance
- **Public bathrooms & signage:** Every public/commercial/civic building generates accessible restrooms with Male/Female/WC relief plaques and exterior identity signage above main approaches.
- **Guaranteed doorway clearance:** Explicit `DoorAxis` plus a mandatory `3×3` jamb-and-apron stencil during generation eliminates T-junction rotated doors and perpendicular wall edges intruding into doorways.

---

## 3. Built for extensibility (from new shops to a Traveling Circus)

Rather than hardcoding every district and building into core Rust enums, generation separates **data declarations** from **reusable spatial solvers**:

1. **Unified `ParcelPlan` replaces single-building parcels and `ArmySub`:** Every parcel resolves into `0..N` independent `PlacedStructure`s (each with its own pad elevation, floor count, roof style, and wing cutaway mask) plus an `OutdoorLayer` of paths, fences, lights, and multi-tile `SetpieceInstance`s.
2. **Declarative definitions (`serde` RON/TOML):** Adding a **Fire Station**, **School**, **Shopping Strip**, or **Traveling Circus** means authoring data specs (`AreaArchetypeDef`, `BuildingArchetypeDef`, `PropDef`, `RoomFurnishRule`, `OutfitVisualDef`) embedded offline via `include_str!` (with optional local folder overrides on desktop for rapid modding).
3. **Circus / Fairground example:** Uses a `"radial_midway"` compound planner on a `48×48` rural arterial clearing to place ticket booths, open-front midway game stalls, backstage trailers, barred animal cages (reusing cell-bar materials), a slashable-canvas octagonal **Big Top Tent** (`ConicalTent` roof that Bayer-dissolves on entry), a towering animated **Ferris Wheel** setpiece (static A-frame base colliders + deterministic tick-driven rotating wheel/gondola instances), and `CircusClown` zombie outfits.

---

## 4. Phased implementation roadmap

Work splits cleanly into six incremental, testable slices:

| Phase | Scope | Key verification check | Status |
| --- | --- | --- | --- |
| **Phase 1: Foundation fixes, sparse `8F` world & forward-compatible coords** | Explicit `DoorAxis` + `3×3` doorway clearance invariant; sparse per-chunk floor storage (`MAX_COUNTY_FLOORS = 8`); per-cell stepped wing roofs & per-wing cutaway masks; deterministic 64-bit UIDs + chunk-local meshes around a floating `render_origin`. | Swept `0.23m` capsule test passes every generated doorway across 64 seeds; 6-floor test structure renders, cuts away per wing, and collapses cleanly across all floors. | Done |
| **Phase 2: Multi-tile props, containers, signage & `Read` action** | Add `PropAnchor`/`PropSpan`, `CellAttachment` layers (`CounterTop`, `WallMount`, `Ceiling`, `Rooftop`), and `HighBlocker` cover. Cache offline-converted meshes and parametric modules in `PropMeshLibrary` and instance closed and opened loot containers. Implement 3D `5×7` glyph and pictogram signage in `signage.rs` and a reader modal for signs, notes, and journals. | Conversion reproducibility, host renderer checks, optimized host benchmark, browser Birch/county Read flows, and authoritative text transport pass. Gallery approval, collider fit, cutaways, furnished live-scene QA, Android county Read, deferred Android Birch Read, and handset validation remain open. Generation stays gated. | In progress — host/browser evidence recorded; review/device gates open |
| **Phase 3: Extensible building grammar, organic towns, highways & Houses/Retail** | `ParcelPlan` & `BlueprintCell` replacing `ArmySub` and `midx/midy` boxes (retiring Army Base until tanks/planes); **organic town block accretion** (split-bank riverfront, ribbon, bay-hugging, cul-de-sacs, separated subdivisions, freeways & roadside gas stations); 5 **House** sub-archetypes + **Grocery Store**. | Towns break out of rectangular grids with varied morphologies, separated neighborhoods, and highway gas stations; houses and grocery aisles read and play distinctly. |
| **Phase 4: Civic landmarks, keycards/breaching & elevators (Police + Hospital)** | Security-zoned **Police Station** (lobby, booking barrier, bullpen, interrogation, barred cells, armory, sallyport); `4–6F` **Hospital** (podium + tower, ambulance bay, ER, pharmacy, wards, ORs); keycard drops + tiered door breaching; `ElevatorCar` physics + **HUD floor button strip (`1F..NF`)** + arrival bell noise magnet. | High-security doors enforce keycard/breach rules; hospital generates 4–6 traversable floors via stairs and button-strip elevators; arrival bell attracts floor threats. |
| **Phase 5: Distinct zombie rigs, outfits & room ecology** | Indexed `generate_zombie_mesh.py` export for `Bloater`, `Armored`, and firearm `Shooter` poses; `MaterialFlags::TINT_EXEMPT` single-batch uniform dyeing; `ZombieSocket` headgear/props; `Screamer`/`Crawler`/`Spitter` behaviors + room-level spawn tables. | Police zombies wear caps/navy or riot gear, hospital zombies wear gowns/scrubs/masks, bloaters/shooters have unmistakable silhouettes at 2:1 isometric zoom, and instance batch count stays bounded on Android. |
| **Phase 6 (Follow-up): Compound showcase decision (e.g. Traveling Circus)** | Revisit compound showcase once the core 4 building families are complete: wire `AreaArchetypeDef` + `CompoundPlanner` + animated `SetpieceInstance`s for a **Traveling Circus** (Big Top, midway stalls, Ferris wheel, clown zombies) + handset 60 Hz validation. | Adding a multi-building compound with custom setpieces works through declarative archetype definitions + pluggable mesh/roof hooks without touching `CountyStage` chunk plumbing. |

Doorway clearance verification is complete in the live CountyStage. The command `cargo test -p zombiebox-core --lib generated_doors_and_gates_pass_runtime_bilateral_capsule_sweep -- --nocapture` passed every generated door and compound gate across seeds `0..63`. It swept a `0.23m` capsule from both normals and checked closed blocking and open passage through materialized walls, props, and chunk-window boundaries. Runtime materialization consumes the recorded `DoorAxis`. The original ArmyBase case was seed `1`, F0 `(102,351)`, with a redundant frontage gate at `(102,350)`. The generator now leaves the frontage open and retains the building-entry gate. The audit also covered the hospital T-junction at seed `5`, F1 `(358,233)`.

Phase 1 host evidence includes the 6-floor and 8F renderer, cutaway, roof, and collision checks. The 3D checker ran on Vulkan llvmpipe and covered Birch outside and inside, a breach, seed 42 residential, downtown, hospital, cabin, and bridge scenes, a hospital revisit, and 180 movement frames across a chunk seam. An offline `zombiebox-ffi` Android-target check also passed. Baseline terrain-seam output with 8 magenta pixels and downhill-aim output with `z=0` versus negative remain recorded. Physical handset and browser movement verification remain open.

Phase 2 host evidence includes 89 renderer GPU library/integration tests and a headless Vulkan/llvmpipe `check3d` run (18,482–86,793 triangles, 15–42 instances, 8–11 of 25 chunks). Core verification also records 72 unit tests including the 64-seed doorway sweep, final action 19/19, host C ABI tests, and TypeScript checks. The optimized host benchmark and browser Birch/county Read checks pass; gallery approval, furnished live-scene QA, Android county Read, deferred Android Birch Read, and handset validation remain open.

Additional integration evidence: EventDriven ownership tests pass for seam-crossing insertion/destruction (exactly the anchor-owner chunk dirties) and container open (zero chunk rebuilds with persistence on revisit); all three `living_props` tests pass. Browser sign open/close and immediate reread pass for Birch and county seed `42`. The optimized static mesh benchmark measured 6 fixtures / 1,278 triangles at p50 36µs and p95 39µs, and 32 / 4,138 at p50 102µs and p95 114µs (719µs and 247µs maxima). It does not replace furnished live-scene or handset pacing checks. Android release compilation passes; the responsive lower-resolution emulator reached Birch gameplay but did not surface the Read prompt, so Birch Android Read is deferred and Android county/handset validation remain open.

---

## 5. Resolved design decisions

1. **Elevator floor button strip (`1F..NF`):** Inside a powered elevator, display a compact **floor button strip** on the HUD so the player has direct, unambiguous floor selection across `4–6+` story buildings.
2. **Keycards/keys & physical breaching for high-security rooms:** Locked doors on **Police Armories, Holding Cells, Booking Gates, and Hospital Pharmacies** require either a matching **key/keycard** (looted from officer/doctor zombies, desks, or cabinets — often hinted at in readable logs) or **physical breaching** (wood/barred doors yield to sustained melee/shotgun; reinforced steel armory doors require the keycard or heavy explosives).
3. **100% diegetic in-world signage + `Read` action (`InteractPrompt::Read`):** Skip automatic floating zone banners; keep signage physical in the 3D world and add a **`Read` action** when facing written objects — covering directional signs, hospital directories, bulletin boards, **notes, journals, clipboards, and environmental lore**.
4. **Focus on the core 4 first; defer Army Base until tanks & planes:** Retire the current Army Base from county generation for now (bringing it back bigger and better alongside **tanks and planes**), focus Phases 1–5 on **Houses, Grocery Stores, Police Stations, and Hospitals**, and then revisit the **Traveling Circus** as our first showcase compound!
5. **Reuse ordinary props and generate layout-dependent geometry.** Start with Kenney Furniture Kit for houses, public restrooms, and offices. Evaluate individual medical and exterior models against the same muted palette and recognizable silhouettes. Keep checkout lanes, shelving runs, curtains, bars, and signage configurable. [Prop asset catalog](prop-asset-catalog.md)
