# Phase 3 implementation plan

Build a county with recognizable, lived-in houses, working retail spaces, and streets that follow the terrain. Birch Street sets the bar for furnished rooms, clear approaches, readable silhouettes, and destructibility. Phase 3 delivers five house families, an open-span grocery, a highway gas station, and terrain-aware towns. Use the current 512 × 512 county for this phase.

Companion docs cover the [living buildings roadmap](living-buildings-plan.md), [building generation](building-generation-algorithms.md), [assets and ecology](building-assets-and-ecology.md), [prop catalog](prop-asset-catalog.md), [town layout](world-and-town-layout-plan.md), and [product decisions](product-decisions.md). This copy was reviewed against [main at 4c60c6f](https://github.com/naclonts/zombiebox/tree/4c60c6fa95410c326d54b354bf63ce91c44baed8). Relative links assume placement in the repository's `docs/` directory.

## Scope and rollout

1. **Prove one county house in play.** Put a furnished Craftsman on an existing lot in a repeatable county fixture. Use the actual streaming, collision, interaction, and rendering paths. Add an L-wing ranch on the same street to test reuse and a small two-floor fixture to test stairs, support, and cutaways.
2. **Fill out the building roster.** Add the Colonial, stepped split-level, duplex, grocery, and gas mart. Use fixed lots for isolated authoring and preview before the new town planner assigns them.
3. **Build one connected town.** Start with a highway ribbon, a residential street or cul-de-sac, and actual foot and motorcycle routes. Add crossroads, split-bank, and bay/lakeshore morphologies in follow-up packages within Phase 3.
4. **Switch county generation after verification.** Keep current police, hospital, downtown, and cabin content reachable through a legacy adapter. Retire the Army Base from active county generation together with its landmark, lot, routing, and runtime special cases. Phase 4 owns civic replacements, Phase 5 owns new infected variants, and later milestones own the circus, freeway interchanges, and infinite-region streaming. Phase 3 uses a state highway as the first regional spine.

Use an opt-in county fixture while the furnished-scene and device gates are open. Keep the normal game playable between packages. The fixture selects data at the generation entry point and feeds the same materializer as the county.

> The current testing override makes the Craftsman–Ranch reference street the default county entry, with the single Craftsman as a fallback. Select `CountyFixture::ColonialLot` to review the Colonial on an existing residential lot. `CountyFixture::BlueprintLot` remains available for the WP 3.1 contract fixture, and `World::legacy` keeps the old baseline for comparisons. The full retirement and version bump remain WP 3.11.

## 1. Ownership and shared contracts

### Module ownership

| Module | Responsibility |
| --- | --- |
| `world/parcels.rs`, `world.rs` | Parcel dispatch, planned pad bounds, lazy immutable detail, chunk-facing views, generation version |
| `world/blueprint.rs` | Sparse local blueprint, coordinate transform, doorway and support validation |
| `world/massing.rs`, `world/floorplan.rs` | Footprint masks, authored room arrangements, fit, windows, stairs |
| `world/furnish.rs` | Fixture placement, dependencies, circulation reservations, final reachability |
| `world/archetypes/` | Static house and retail definitions and parcel composition |
| `world/geography.rs`, `world/town_planner.rs`, `world/relief.rs` | Terrain suitability, road topology and grades, zoning and lots |
| `world/dressing.rs` | Outdoor surfaces, paths, parking, frontage, signs, vegetation |
| `action/county.rs` | Materialize into live cells, structures, props and signs. Apply existing chunk deltas |
| `action/prop_catalog.rs`, `zombiebox-render3d/src/mesh/{library,kit}.rs` | Authoritative gameplay prop definitions, shared visual prototypes |

Repository paths in this plan are relative to `crates/zombiebox-core/src/` unless a crate is named. The Rust sketches specify ownership and fields. Map their conceptual types onto repository types during WP 3.1 and record the resulting names and entry points in the handoff. Use static Rust definitions in Phase 3. A new archetype should register through one dispatch point and materialize through the common path.

```rust
struct ParcelPlan {
    id: ParcelId,
    bounds: GridRect,               // Half-open world-tile rectangle
    frontage: Frontage,             // Cardinal side, usable interval, road/access node
    zone: ZoneKind,
    structures: Vec<PlacedStructure>,
    outdoor: OutdoorLayer,
}

struct PlacedStructure {
    id: StructureId,
    archetype: ArchetypeId,
    transform: LotTransform,
    foundation_z: f32,
    blueprint: BlueprintGrid,
}

struct OutdoorLayer {
    surfaces: Vec<SurfacePatch>,    // Sidewalk, driveway, parking, apron, porch path
    paths: Vec<AccessRoute>,        // Links frontage, doors, parking and service access
    fences: Vec<FenceSegment>,
    props: Vec<PlannedProp>,
    signs: Vec<PlannedSign>,
    vehicle_slots: Vec<VehicleSlot>,
}

trait CompoundPlanner {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError>;
}

trait StructureGenerator {
    fn generate(&self, context: &StructureContext) -> Result<BlueprintGrid, FitError>;
}
```

- **Pads and timing.** Derive a stable footprint and planned pad bounds before final relief. Finalize foundation elevations before placing props, signs, or other things that sample surface height. Multi-structure parcels carry independent pads where needed. Keep the existing lazy parcel detail and chunk ownership strategy. Write the expanded blueprint once per parcel and let all intersecting chunks read from it. Keep the live world on sparse floors. Format static definitions and cached detail so a lot can be inspected without creating all other interiors.
- **Coordinates.** Use local `u` along frontage and `v` into the lot. Define world placement as `origin + u * right + v * inward` with a cardinal basis. Define origin as the world tile for local `(0,0)`. Transform all rectangle corners or covered cells, then take min/max in world axes. Convert legacy inclusive lot bounds once at the adapter. Test all four frontage orientations using an asymmetric fixture. Rotate door tangents, stair vectors, sign normals and facing by the same basis. For multi-tile props transform their covered tiles, derive the world min corner and world span, then use the existing pivot and support offset. Apply each rotation once.
- **IDs and replay.** Derive versioned, namespaced 64-bit IDs from canonical world-space parcel identity, stable structure slots and authored local keys. Derive independent streams for roads, layout, furnishing, loot, signs and dressing using seed, owner ID and stage salt. Use a stable candidate order, explicit score tie-breaks, and canonical ordering when emitting or hashing cells, placements, and roads. Keep lookup maps out of iteration-dependent decisions. Test different chunk visitation orders and verify that changing loot or optional clutter leaves the structure, openings and road plan identical. Use a documented stable mixer and bump the existing `GENERATOR_VERSION` when enabling the new baseline. Keep incompatible saved deltas with their original generator if supported, or use the existing version-rejection path. Verify actual save behavior before cutover.
- **Chunk ownership.** A parcel supplies all intersecting geometry, but each persistent entity has one owner. Place props through the existing `PropAnchor` path and use the anchor-owner chunk. Query its covered cells across seams through the same runtime spatial APIs. Keep structure IDs separate from transient renderer IDs. Revisit must preserve opened containers, broken props, structural edits and moved vehicles without duplication.

### Blueprint and runtime handoff

```rust
struct BlueprintGrid {
    cells: BTreeMap<LocalCell, BlueprintCell>,  // Key includes floor index
    rooms: Vec<RoomMeta>,
    doors: Vec<DoorIntent>,
    stairs: Vec<StairLink>,
    props: Vec<PlannedProp>,
    signs: Vec<PlannedSign>,
}

struct BlueprintCell {
    room: Option<RoomId>,
    wing: WingId,
    kind: TileKind,
    material: Material,
    surface: Surface,
    indoor: bool,
    load_bearing: bool,
    stair_dir: Option<CardinalDir>,
    roof: Option<RoofSpec>,
    reserved: ReserveFlags,
}

struct DoorIntent {
    cell: LocalCell,
    tangent: DoorAxis,
    kind: DoorKind,
    connects: (SpaceId, SpaceId),   // Room, exterior access, or separate duplex unit
}

struct PlannedProp {
    key: LocalPlacementKey,
    prop: PropId,
    anchor: LocalCell,
    facing: PropFacing,
    room: Option<RoomId>,
    host: Option<LocalPlacementKey>,
    offset_m: Vec3,
    loot: Option<LootProfileId>,
}
```

- Use existing `DoorAxis::{AlongX, AlongY}` as the **wall tangent** and get the normal from the runtime helpers. One intent owns each opening. Compile openings and windows to existing runtime cell semantics. Reject inconsistent duplicate intents. Store the physical walls and openings in one authoritative representation and derive room adjacency from the compiled openings.
- Store room membership by cell mask where spaces are irregular. Room metadata needs room kind, optional duplex unit, wing, and required interaction approaches. Stairs link explicit floor/cell endpoints and reserve the landings. Floor indices are `0..MAX_COUNTY_FLOORS`, where `MAX_COUNTY_FLOORS = 8`. Use existing roof/support fields and preserve per-wing cutaways in the compiled result.
- County materialization writes `TileKind`, `Material`, `Surface`, indoor state, building ownership, support, door axis, stairs and roofs. Props and signs go through Phase 2 placement and Read interfaces. A porch or canopy can have a roof and supports while remaining outdoor space. Record the support that carries each stepped roof or upper floor. Route shelf cover, collisions and destruction through core.
- `FitError` distinguishes invalid lot, impossible required room or fixture, access failure, and support failure. Candidate searches have fixed attempt budgets. On exhaustion, use an authored fallback for that lot class, select a compatible archetype where allowed, or leave an optional parcel unbuilt. Required landmarks reserve feasible parcels and use a verified fallback. Include reproducible seed, parcel, orientation, floor and invariant in failure diagnostics.

## 2. Algorithms and authored layouts

### Residential massing and room fitting

1. Subtract setbacks and reserve paths from frontage to porch or driveway before choosing massing. Fit a main rectangle plus an optional porch or garage rectangle. Upper floors occupy the supported overlap of their lower masses. Compose masks in local coordinates. Keep porch and garage roof silhouettes distinct from the main house.
2. Choose an authored **room arrangement** that fits the available width/depth. Each arrangement declares entry side, zones, required adjacency, exterior exposure, room minima and stair position as applicable. Carve the entry, hall or shared living zone, and stairs first. Subdivide remaining rectangular bays with constrained axis-aligned cuts. Enumerate feasible cuts from room minima and required wall contacts. Include usable spaces for signature fixtures when sizing bays.
3. Place doors on shared walls that have the required tangent and approach space. Prefer long, clear segments with midpoint bias and deterministic ties. For a door at `p`, keep secure jambs at `p ± tangent` and clear walkable passage on both normals. Run the shared 3×3 clearance stencil before emitting walls, stairs or fixtures into those cells. Audit widened transitions with the same runtime capsule. A failure retries an alternate opening, cut or layout within the budget.
4. Place windows on eligible exterior walls, preserving privacy, structure and fixture candidates. Kitchen sink under a window is a preference. Check room reachability from the appropriate public entrance. Private bedrooms and bathrooms cannot be through-routes. Check exposed roof, roof support, and stair alignment and landings before furnishing.
5. Choose exterior variation with the building's era and use in mind. Share a neighborhood palette and setbacks while varying porch depth, window groupings, trim and yard treatment per lot. Keep changes to material palette independent of circulation. Review adjacent houses from the road at gameplay zoom before adding more variations.

| Family | Ship these layout options | Invariants and visual cues |
| --- | --- | --- |
| Craftsman bungalow | **A** porch → living → dining/kitchen, with a short bedroom hall. **B** porch → foyer between living and dining, with kitchen at rear and bedrooms off a side hall. | One floor, street-facing porch with usable approach, recognizable trim and porch supports. |
| L-wing ranch | **A** garage → mudroom → kitchen, living toward street. **B** garage → utility/kitchen entry, living along rear yard and bedrooms on short side hall. | Driveway connects to the garage opening and aligns with the garage wing. Public walking approach stays clear of vehicle placement. |
| Center-hall Colonial | **A** straight hall with living and dining on opposite sides and stair along the hall. **B** compressed hall with a rear kitchen and side stair reached directly from the hall. | Two floors, front-centered entry, supported upper mass, stairs from common circulation to the upstairs hall. |
| Stepped split-level | **A** one-floor street living wing and taller rear bedroom wing. **B** same massing with taller side wing and stair in the shared entry zone. | Whole-floor changes using existing stair mechanics, supported stepped roofs, per-wing cutaway. |
| Side-by-side duplex | **A** paired street doors. **B** twin approaches with offset front stoops and a shared frontage path that forks outside. | Two independent homes, uninterrupted party wall, separate kitchens and baths, unit-specific reachability and room ownership. |

### Furnishing and navigation

Reserve exterior approaches, door aprons, stair landings and interaction positions. In each room, connect every required portal and approach to a chosen circulation node using deterministic four-neighbor BFS on the walkable mask. Enforce clear corridor width on the reserved corridor and check the resulting route again after furnishing. Start with one clear tile in houses and two in ordinary public aisles, and use the existing runtime capsule for physical clearance.

```rust
struct PropPlacementRule {
    key: RuleKey,
    alternatives: &'static [PropId],
    strategy: PlacementStrategy,
    required: u8,
    maximum: u8,
    extra_chance: f32,
    priority: u8,
    loot: Option<LootProfileId>,
}

enum PlacementStrategy {
    WallRun { preference: WallPreference, segments: (u8, u8) },
    AisleRun { clear_gap: u8, end_margin: u8, endcap: Option<PropId> },
    RelativeTo { parent: RuleKey, offsets: &'static [LocalPropOffset] },
    FloorOpen { min_wall_clearance: u8, prefer_corner: bool },
    Attachment { layer: CellAttachment, host: RuleKey },
}
```

1. Order by dependency first. Among ready candidates, place required fixtures and their support hosts before optional clusters or clutter. Break ties by descending priority and then stable rule key. Reject cycles during validation. A child with an optional absent parent is skipped. A required missing host fails the layout candidate.
2. Enumerate wall segments, aisle centerlines, or floor cells in stable order. Rotate `PropDef.span` and test the whole footprint and collider envelope. Reserve windows for lighting where requested. Keep clear sweep space for container states and a reachable interaction position. Attachments validate their host surface and vertical bounds. Their horizontal overlap with the host is intentional, and they must fit without blocking the approach.
3. Use relative offsets from the placed parent's pivot and rotated bounds. Derive chair locations around the actual edge of the table. Prefer a TV on the stand opposite the sofa, with a coffee table between and a clear walking lane around the seating group. Use typed `PropId` alternatives for bed sizes. Match the actual selected parent through its placement key.
4. Search a small number of deterministic arrangements if required fixtures fail. Validate a fallback with the same tests. Skip optional clutter when it would close a route and record the reason in fixture diagnostics. Keep contextual loot overrides in instance data only where the runtime supports them. Model closed/opened as container state, and use container-capable IDs. Where an asset has no container behavior, put loot in a nearby supported crate or box until gameplay support is added.

| Room | Required fixtures and placement | Optional variation |
| --- | --- | --- |
| Kitchen | Sink, stove and fridge on usable wall run, with an open interaction approach. Prefer sink under window. Use base cabinets to fill continuous gaps. | Small dining group where it fits, countertop clutter, pantry box. |
| Living room | Sofa plus usable seating area. Space for facing TV and coffee table with a path around the group. | TV stand/module, attached TV, bookcase, rug and readable note. Use a stand that supports the TV if it is placed. |
| Bedroom | Bed against a wall with accessible approach. Choose double or single by actual rotated footprint. | Nightstands, wardrobe, desk with chair, journal. |
| Bathroom | Toilet and sink with reachable fixture approaches. A bath or shower in each primary residential bathroom. | First-aid kit on a compatible host. A compact secondary WC may omit a bath. |
| Garage and porch | Keep driveway/garage opening and public porch path clear. Anchor porch rails to exposed edges. | Crates, unoccupied parking position, porch chairs and mailbox. |

Use these initial rule bounds for the residential fixture. Counts apply per eligible room. Each placement still goes through the full span, host and approach checks.

| Group | Starting ranges and placement order |
| --- | --- |
| Kitchen wall run | Place 1 sink, 1 stove and 1 fridge before 2–5 cabinet segments where the wall fits. Try the sink/window alignment first, then a clear adjacent wall. For a dining group, place 1 table and sample up to 4 chairs on its open sides. |
| Seating group | Try 1 sofa against an interior wall, 1 facing stand and 1 attached TV, then 1 coffee table. Place up to 2 bookcases and 2 boxes only after the circulation check. |
| Bedroom cluster | Place 1 bed and try up to 2 nightstands using the selected bed's bounds. Add at most 1 wardrobe and 1 desk/chair cluster. Use the desk's supported opened state if searchable. |
| Bathroom and entry | Place 1 toilet and 1 sink, then a primary bath fixture with a usable entry. Fit the bath footprint before accepting the room dimensions. Try at most 1 attached first-aid kit. Reserve the porch entry and garage approach before trying 0–2 porch chairs or up to 4 garage crates. |

### Grocery and gas station

**Grocery.** Reserve a wide arterial-facing parcel with front parking and an independent rear service approach. Start with one floor. Lay out a shallow vestibule and checkout zone, open sales floor, and rear stockroom/public restrooms. A front strip around 4 tiles and rear strip around 6 tiles are starting points subject to actual clearance. Allocate a walkable connection from the sidewalk and parking to the main entrance. Position a cart bay out of the vestibule path and reserve a separate loading approach.

- Reserve main cross-aisles at 3 clear tiles and shelf aisles at 2 before placing pillars or shelf modules. Use a repeating 6-tile starting pitch for load-bearing columns. Align columns with shelving bays where feasible and check support and navigability after final placement. Skip a shelf segment where a column, door approach or cross-aisle needs the space.
- Build checkout rows with `PropId::CheckoutLane` on the floor and `PropId::CheckoutTill` attached to a compatible countertop location. Use `PropId::GondolaShelf` modules for repeatable 1.45 m tall cover. Use endcaps and perimeter coolers to give the store a recognizable layout. Place accessible stockroom boxes for grocery loot. Test line of sight and cover through the real colliders.
- Provide an exterior identity sign, aisle markers, a WC plaque, and a Read interaction for at least one store notice. Use the actual public route from door to sales floor, checkout and restroom in acceptance. Check the separate rear route to stockroom and loading dock.

**Gas station.** Use one parcel with two placed structures, the mart and open-sided canopy, plus driveway apron, signs and pump islands in the outdoor layer. Start from a roughly 22 × 18 tile lot and expand or pick a larger lot if the route and fixtures do not fit. Connect the highway to an entry and exit or a validated turnaround. Prove a drivable path past one pump island with parked vehicles in their permitted slots. Keep a distinct walking route to the store. Inside, place a checkout lane or clerk counter and attached till, a short snack aisle, perimeter coolers and a public WC. Put the price pylon outside the driving envelope. Use separate parking slots for static props and drivable vehicles and route drivable placement through existing vehicle ownership.

### Town geography and frontage

1. **Preliminary natural terrain.** Separate or expose a deterministic natural-height, water and slope sample from the current relief pipeline. Today `World::new` plans roads and pads before final relief. First produce the natural suitability field, then select routes and pads, then apply final road grades, bridge decks, and foundation blending. Reject or reroute access if final grades or transitions fail runtime checks. Keep world size and chunk coordinates in Phase 3 stable.
2. **Connected roads.** Start with a highway ribbon and a collector to a residential street or satellite cluster. Represent the network as connected nodes and edges with road class, centerline, width and bridge spans. Start with arterial, collector, local and trail classes. Route necessary connections on a coarse grid, such as 16 tiles, with explicit stable queue tie-breaks. Expand and grade the route on the fine grid, then validate the full paved corridor. Cross rivers at short validated bridge spans with connected approaches. Keep unbuildable sites and water crossings out of ordinary road candidates.
3. **Grow blocks along roads.** Expand from the commercial core through candidate frontages scored by flatness, dry buildable area, distance from center and connection to the arterial. Increase density near the core and break up the fringe with T-junctions, park space and a turnaround. A detached residential pocket gets a collector branch, trees or fields along the approach, and a real route back to town. Keep lot placement subordinate to the road network.
4. **Split usable frontage.** Keep lots cardinal and use snapped cuts perpendicular to the frontage for standard through-depth splits. Require each accepted child to retain a minimum road-facing interval and sufficient buildable area after setbacks. Reject landlocked remainders. Begin with residential lots around 12–18 tiles wide and 14–22 deep, with at least 8 tiles of usable frontage. Grocery and gas lots use their own fit checks. Validate road crossings and frontage on the final raster and require non-overlapping dry lots.
5. **Assign landmarks and variants.** Reserve sites for the starting residential district, grocery, gas station, and the existing downtown, police, hospital and cabin content. Keep civic sites near the downtown–residential seam and connected to arterials. Choose crossroads where two arterials meet, split-bank where both banks support development and a bridge, and bay/lakeshore where a dry setback and feasible grades allow a contour road. Fall back to a viable morphology if terrain rejects a candidate. Maintain local fixtures that force each morphology for review.
6. **Validate access.** Run road connectivity from the starter approach through every required district. Continue on foot from roads to actual public doors. Check swept motorcycle routes over the primary highway, a bridge when present, turns, grade transitions and both retail approaches. Inspect the final route with vegetation, signs, parked vehicles and sidewalk dressing applied. Keep regional town stops sparse and purposeful. A small subdivision need not contain grocery or fuel services of its own.

## 3. Asset reuse and placeholders

The checked-in [converted manifest](../assets/living-buildings/converted/manifest.json) lists 34 prototypes or states at the review commit. Gameplay uses [`PropId` and `PropDef`](../crates/zombiebox-core/src/action/prop_catalog.rs). The [mesh library](../crates/zombiebox-render3d/src/mesh/library.rs) selects the appropriate prototype for each state. Build the registry check against those interfaces.

| Reuse | Application |
| --- | --- |
| Existing Phase 2 furniture and fixtures | Sofa, coffee table, table/chairs, beds, desk, bookcase, TV, kitchen modules, fridge, bath fittings, crates, boxes and static cars. Check gallery-scale fit, facing and support. |
| Procedural `GondolaShelf` and `CheckoutLane`, existing `CheckoutTill` | Compose retail aisles and lanes from the existing geometry. Attach the till through the existing countertop layer. |
| Existing mailbox, fence and street dressing | Reuse its IDs, authored geometry or existing dressing path where fit has been verified. Adapt rails from the edge/fence system where possible. |
| Phase 2 signs and Read | Identity signs, WC pictograms, aisle marks, notes and price pylon text. Use the existing sign geometry and readable ID transport. |

For a true gap, add one stable gameplay ID and a searchable visual TODO in the same package that needs it. `PropDef` owns span, physical bounds, collision, cover, materials and gameplay capability. The fallback visual uses those same bounds and the mesh library's normal pivot and facing. Use a distinct muted tint in preview, and verify that swapping the mesh leaves gameplay placement, cover and persistence intact. Preferred assets should have readable silhouettes and muted palettes at gameplay zoom.

| Gap | Starting size in meters (width × depth × height) | Initial handling |
| --- | --- | --- |
| Gondola endcap | 1.0 × 0.9 × 1.45 | Adapt shelf kit geometry, keep `HighBlocker` when the collider warrants it. |
| Glass cooler/freezer | 2.0 × 0.9 × 2.0 | Perimeter module with visible panel framing, high cover. |
| Stockroom rack | 2.0 × 1.0 × 2.2 | Shelf-kit variant or placeholder, high cover. Keep loot in supported containers until rack interactions exist. |
| Fuel pump | 0.9 × 0.7 × 1.65 | Placeholder with a recognizable dispenser silhouette and a tested driving envelope. |
| Convenience counter | 2.0 × 0.9 × 1.05 | Reuse the checkout lane or counter when suitable, or add a countertop-compatible module. |
| Wardrobe | 1.0 × 0.6 × 1.85 | Optional tall fixture. Reuse cabinet geometry if fit matches. |
| Nightstand | 0.5 × 0.5 × 0.6 | Optional bedside module. Prefer an existing table or cabinet when suitable. |
| Porch rail | 1.0 × 0.15 × 0.95 | Edge-mounted module. Leave porch entry and stair apron clear. |
| Dumpster | 2.0 × 1.2 × 1.35 | Optional service-yard cover. Choose cover class by runtime behavior and test approach. |

## 4. Agent work packages

Give each agent its package, the shared contracts above, and the previous handoff. Each package should compile after merge. Tests and fixtures travel with the code they exercise. A package owns its listed files and may make minimal module-registration changes. Coordinate overlapping ownership in merge order. In handoff, record the fixture selector, seeds and orientations, actual commands and results, new interfaces, and any open findings. Use named targeted tests for intermediate merges and the workspace suite for final sign-off.

| WP | Outcome | Depends on |
| --- | --- | --- |
| 3.1 | One county lot streams a blueprint through the normal runtime | — |
| 3.2 | Furnished Craftsman, playable at the first milestone | 3.1 |
| 3.3 | Ranch on the same street and two-floor support fixture | 3.2 |
| 3.4 | Reusable layout fitting and the Colonial | 3.3 |
| 3.5 | Stepped split-level and duplex | 3.4 |
| 3.6 | Open-span grocery | 3.3 |
| 3.7 | Two-structure gas station | 3.6 |
| 3.8 | Natural terrain input and one connected highway-ribbon town | 3.3 |
| 3.9 | Retail lots, frontage splitting, regional access and outdoor dressing | 3.5, 3.7, 3.8 |
| 3.10 | Crossroads, split-bank and lakeshore variations | 3.9 |
| 3.11 | County cutover, Army Base retirement and release verification | 3.10 |

WP 3.8 can run in parallel with retail after WP 3.3. Keep housing and retail PRs independent where possible. Serialize any changes to shared furnishing, archetype registration or catalog files.

### WP 3.1 — Blueprint-to-county handoff

Handoff: [WP 3.1 handoff](phase3-wp31-handoff.md).

- **Owns** `world.rs`, `world/parcels.rs`, new `world/blueprint.rs`, the registration seam in `world/archetypes/mod.rs`, and `action/county.rs`.
- **Build** an opt-in existing-lot fixture with one authored blueprint room, one exterior door and one known Phase 2 prop. Add parcel dispatch, a local-to-world transform, and one materialization entry point. Keep the legacy parcel fallback. Connect the blueprint to planned pads, live floor cells, support/roof fields, explicit door axes, and stable prop and building ownership. Start with the data required by this fixture and add shared fields as downstream packages prove their use.
- **Accept** an asymmetric fixture in all four orientations. Check opening tangent, rotated footprint and pivot, roof/structure ownership, and the runtime bilateral door sweep. Force a seam-crossing prop and reverse the order of streaming the neighboring chunks. Compare owner, footprint and placement IDs. Revisit the fixture and check exactly one prop instance.
- **Hand off** the fixture selector, coordinate examples including an even-sized span, and the names of the parcel, placement and validation entry points.

### WP 3.2 — First furnished Craftsman

Handoff: [WP 3.2 handoff](phase3-wp32-handoff.md).

- **Owns** `world/archetypes/houses.rs`, `world/furnish.rs`, and the first residential fixture.
- **Build** arrangement A with the current county frontage, main mass and porch, one furnished kitchen, living room, bedroom and primary bath. Begin with authored room masks and deterministic wall/relative placement using existing IDs. Include one supported searchable container and one readable. Use the seed-based decision and placement rules in Sections 1–2.
- **Accept** live entry and room traversal in isometric and a street-level camera, reachable fixture interactions, correct cutaways, and the bilateral capsule sweep. Open a container with zero chunk rebuilds, breach a wall, leave streaming range and revisit. Record that both states persist. Capture a street view and a furnished interior from the same seed.
- **Hand off** a working fixture plus the prop approach and room-circulation APIs. Record fit or device findings for the release review.

### WP 3.3 — Ranch and shared street

Handoff: [WP 3.3 handoff](phase3-wp33-handoff.md).

- **Owns** `world/archetypes/houses.rs`, `world/massing.rs`, residential fixture dressing and the small two-floor fixture. Extend `world/furnish.rs` as needed for shared placement.
- **Build** the ranch garage wing and driveway, place it next to the Craftsman, and add the second arrangement for each family. Write the smallest shared footprint-mask helper that supports both. Keep vehicle and pedestrian reservations separate. Use the two-floor fixture to wire aligned stairs, landing reservations, roof and supporting cells into the live path.
- **Accept** street-side recognition of the two houses by approach, massing and roofline. Walk the garage connection and both directions on the stairs. Verify support loss follows existing collapse rules and entering a wing cuts away the correct roof. Confirm the two dwelling entries remain clear with a parked car.
- **Hand off** the first reference street for visual and performance comparison.

### WP 3.4 — Room-fit solver and Colonial

Working plan and host review are recorded in the [residential handoff](phase3-wp33-handoff.md#wp-34-room-fitting-and-colonial).

- **Owns** `world/floorplan.rs`, residential `world/massing.rs` and `world/archetypes/houses.rs`.
- **Build** the bounded arrangement selector, bay cuts, common-space and landing reservations, and shared-wall opening candidates. Convert the first two house families to use these helpers where the fit remains equivalent. Add both Colonial layouts and their supported upper floors. Use a compact authored fallback when a feasible lot cannot fit the preferred arrangement.
- **Accept** multi-seed and four-orientation fit for representative small, typical and large compatible lots. Verify required fixtures, exterior access, public-to-private circulation, two-way stair traversal and fallback behavior. Assert that the layout is stable when loot changes.

### WP 3.5 — Stepped split-level and duplex

- **Owns** residential definitions and the house family test matrix.
- **Build** both arrangements per family. Use the shared stair/link contract for the stepped house and distinct unit, room, wing and entry ownership for the duplex. Add any needed small frontage helpers for paired entries.
- **Accept** all five families across seeds `0..63` and four orientations at blueprint level. Verify their archetype-specific topology and required furnishing. Run live stair and wing-cutaway checks for stepped houses and check both duplex entrances and the intact party wall. Replay seam placements and compare normalized plan output.

### WP 3.6 — Open-span grocery

- **Owns** `world/archetypes/retail.rs`, retail fixture, and approved shared prop additions in `action/prop_catalog.rs` and renderer `mesh/{library,kit}.rs`.
- **Build** grocery zones, walkway reservations, pillar grid, floor lanes with attached tills, shelf runs, rear service approach, WC, and signage. Start with an existing road-front fixture lot. Reuse the kit shelf and lane before adding any new visual.
- **Accept** public path from sidewalk and parking to the main entrance, checkout and WC. Verify independent rear access, full clear aisle widths, real shelf occlusion, support behavior and an accessible looted fixture. Check that open till bounds and carts leave the approach usable. Capture interior and frontage views.

### WP 3.7 — Highway gas station

- **Owns** retail compound definitions and gas station fixture. Coordinate with the catalog owner for a pump or counter addition.
- **Build** independent mart and canopy placements on one parcel, paths and pump islands, test vehicle slots, a sign pylon, and a fixture driveway connected to a current road. Share checkout and shelf rules with the grocery. Keep open canopy cells outdoor and emit roof support through the common blueprint.
- **Accept** a drive in, pass a pump, and exit or turn around with normal parking populated. Walk through the public path to the WC, read a sign, approach the till and stream away and back. Verify the two structures and each parked vehicle keep one stable identity. Hand off minimum feasible frontage, depth, and access parameters to WP 3.9.

### WP 3.8 — Natural terrain and highway-ribbon town

- **Owns** `world.rs`, `world/geography.rs`, `world/relief.rs`, and new `world/town_planner.rs`.
- **Build** natural terrain inputs, road graph and raster, highway ribbon, collector and residential cul-de-sac on an opt-in seed fixture. Use explicit tie-breaks, grade and water checks, and retain `ValleyMeta` or its replacement for terrain/bridge code. Put the first residential street on a reachable dry block.
- **Accept** stable road hashes and routes from spawn to the primary arterial and residential street. Drive a motorcycle through the grades, junction and turnaround. Run the existing terrain seam and raycast checks. Record a clean pre-change baseline and diagnose any changed terrain failure before handing off.
- **Hand off** frontage and lot input APIs, road classes, sampling units and route fixture coordinates.

### WP 3.9 — Zoning, retail frontage and dressing

- **Owns** lot subdivision in `world/town_planner.rs`, the outdoor layer consumption in `world/dressing.rs`, and the town fixture.
- **Build** frontage-preserving lots, regional grocery and highway gas sites, park or undeveloped fringe, and the detached residential connector. Preserve connected civic/downtown/cabin lots through the adapter. Stamp sidewalk, driveway, parking, loading apron, safe vegetation, and signs from parcel plans. Keep lots and road corridors disjoint except at explicit access connectors.
- **Accept** across the seed corpus that built lots are dry and non-overlapping, every required landmark is reachable from spawn, and the actual grocery and gas accesses work on foot and by motorcycle as applicable. Rerun after dressing and parking. Verify users can visually distinguish an entrance, a loading dock, and a private driveway.

### WP 3.10 — Other town morphologies

- **Owns** morphology selection and candidate generation in `world/town_planner.rs` and bridge/terrain integration in `world/geography.rs` and `world/relief.rs`.
- **Build** crossroads, riverfront split-bank, and bay/lakeshore layouts on the shared graph, frontage and grading contracts. Use validated bridges for cross-bank access. Preserve dry shore setbacks and useful dead ends. Record eligibility and fallback diagnostics.
- **Accept** fixed review fixtures for each morphology, plus multi-seed topology/access regression. Drive a bridge and its approaches, and traverse dry foot routes on both banks. Inspect terrain seams and use canonical plan hashes to check repeatability. Confirm in a map view that the road hierarchy and town shapes differ visibly.

### WP 3.11 — County cutover and release checks

- **Owns** final gating and `GENERATOR_VERSION` in `world.rs`, legacy parcel dispatch and site cleanup, `action/county.rs`, and county integration checks.
- **Build** default dispatch for Phase 3 houses, grocery, gas and geography after fixture acceptance, and preserve current civic, downtown and cabin content through the adapter. Remove active Army Base landmarks, military lots, special materialization and obsolete tests together. Clean up the 4-quadrant paths only for migrated content. Update seed fixtures that intentionally change, documenting their new coordinates and keeping their assertions about behavior.
- **Accept** all final checks below. Keep the opt-in path until the furnished-scene and handset review is complete. Record generator-version handling, test results and scene evidence in the handoff.

## 5. Final verification

| Check | Pass condition |
| --- | --- |
| Doorways and runtime navigation | Run `cargo test -p zombiebox-core --lib generated_doors_and_gates_pass_runtime_bilateral_capsule_sweep` over seeds `0..63`. Closed openings block and open openings pass the 0.23 m capsule in both directions. Extend the test enumeration to include newly materialized doors and active legacy sites so the gate cannot pass on an empty set. Check room reachability, required fixture approaches, stairs and exterior entries after dressing. |
| Determinism and persistence | Compare canonical roads, structures, placement IDs and loot across repeated runs and chunk load orders. Include the existing `living_props` coverage and the county geography test `county_roads_frontages_and_natural_landmarks_across_seeds` or its successor. Check floor and wing rendering, prop overlap at seams, opened containers, structural destruction, and taken/moved vehicles after leaving and returning. Confirm generator version behavior. |
| Rendering and visual playthrough | Use headless `check3d` or `preview3d` and also walk or ride seed 42 and one alternate seed in the browser. Capture both at street level and normal isometric zoom. From the street, identify individual houses by their rooflines and approaches, find their actual doors, and tell the grocery entrance from its loading dock. Inside, check floor transitions, cutaways, breaches, signs and Read. Include a complete gas station drive-through and at least one bridge or grade transition. |
| Performance | Record host and modest physical Android handset separately. Measure frame pacing during residential walking, dense retail and fast road streaming. Record the 60 Hz target, the static-prop rebuild target below 0.35 ms per dirty chunk, the target below 20k triangles per chunk, and zero rebuilds on container open. Record draw and instance batch counts against the [existing mobile budgets](building-assets-and-ecology.md#4-asset-creation-pipeline--60-hz-mobile-budgets). Verify Read in a furnished County scene. Record deferred Birch Android Read separately from county readiness. |
| Baseline and release | Run `cargo test --workspace` and report the result. Phase 2 records a reproducible baseline `county_seed_42_terrain_seam_and_diagonal_raycasts` failure on the old generator. Reproduce or clear it at the starting commit, investigate any change during terrain work, and record a resolution before calling the workspace suite green. Keep the focused behavior checks even if a terrain fixture must move with the new generator. |

The first milestone is complete when a furnished Craftsman can be entered, explored, looted, breached and revisited through the real runtime. The street with the ranch is the first test that the shared contracts support distinct buildings. The final county cutover demonstrates that the same generation path works for a two-structure roadside parcel, a public retail building and a believable town.
