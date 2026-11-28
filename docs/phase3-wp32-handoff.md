# Phase 3 WP 3.2 handoff — first furnished Craftsman

Status: implemented and verified on host tests. `World::new` now streams the
furnished Craftsman by default; the WP 3.1 contract fixture and the legacy
baseline remain available explicitly.

## Fixture selector

```rust
use zombiebox_core::{CountyFixture, World};

let world = World::new(seed);                          // CraftsmanLot
let wp31  = World::with_fixture(seed, CountyFixture::BlueprintLot);
let legacy = World::legacy(seed);                      // CountyFixture::None
```

- `World::fixture()` reports the active selector.
- `World::fixture_plan()` exposes the selected parcel for previews and tests.
- `CountyFixture::CraftsmanLot` selects the nearest lot to the residential
  spawn that the authored plan can fit, in stable distance/district/coordinate
  order.
- Representative seeds `0, 1, 2, 7, 42, 63, u64::MAX` all select the fixture.
  Seed `42` is the review seed; seed `63` exercises a different optional
  furnishing roll and the fallback that drops blocking optional clutter.

## Names and entry points

| Role | Name |
| --- | --- |
| House generator | `world::archetypes::houses::CraftsmanBungalow` |
| Parcel planner | `world::archetypes::houses::CraftsmanParcel` |
| Fixture helper | `world::archetypes::houses::craftsman_fixture_parcel` |
| Blueprint helper | `world::archetypes::houses::craftsman_blueprint` |
| Structure helper for tests | `world::archetypes::houses::craftsman_structure` |
| Fixture dispatch | `world::archetypes::plan_fixture` |
| Frontage placement | `world::blueprint::place_blueprint_along_frontage` |
| Furnishing entry point | `world::furnish::furnish(seed, &mut blueprint)` |
| Furnishing rule | `world::furnish::PropPlacementRule` (`RuleKey`, strategies, required/maximum, `extra_chance`, priority) |
| Furnishing report | `world::furnish::FurnishReport` |
| Readable/sign projection | `PlacedStructure::signs_world`, `PlacedStructure::planned_signs` |
| Runtime readable handoff | `CountyStage::blueprint_readables`, `ActionGame::sync_county_blueprint_readables` |

## House contract

The Craftsman uses local `u` along the frontage and `v` into the lot. Local
`v = 0` is the porch edge at the street. The blueprint bounds are `13 × 14`
tiles including the porch.

```text
v=0..1    porch deck, posts at u=4 and u=8
v=2       front wall: door at (6,2), living windows at (3,2)/(4,2),
          kitchen windows at (9,2)/(10,2)
v=3..6    living (u=2..6) | wall u=7 | kitchen/dining (u=8..12)
v=7       interior wall with living-hall and kitchen-hall doors
v=8       hall (u=2..12)
v=9       interior wall with bedroom and bathroom doors
v=10..12  bedroom (u=2..6) | wall u=7 | bathroom (u=8..12)
v=13      rear wall
```

- One exterior door, five interior doors, explicit world door axes.
- Rooms: Living, Kitchen, Hall, Bedroom, Bathroom.
- Required fixtures: sink, stove, fridge, sofa, bed, bathtub, toilet and
  bathroom sink.
- Searchable container: the kitchen `Fridge` (the existing prop carries
  medical loot and container behavior).
- Readable: `ReadableId::CraftsmanNote`, a `Note` on the required bedside
  nightstand (`RuleKey(31)`, beside the bed head). `PlannedSign.host` ties the
  note to that rule, and furnishing re-anchors it to the placed nightstand.
- Windows are preserved as lighting/vault candidates. `HighBlocker` props are
  rejected when they would sit in front of a window.

## Prop approach and circulation API

`furnish::furnish` runs after doors and stairs are authored:

1. `reserve_circulation` reserves door aprons, stair landings and deterministic
   four-neighbour BFS spines between each room's portals.
2. Rules are ordered by required-first, dependency depth, descending priority
   and stable rule key. Relative/attachment children therefore never run before
   their host.
3. Wall runs test the full rotated prop span, the wall tangent, the preferred
   wall class and window adjacency. `FloorOpen`, `RelativeTo` and `Attachment`
   cover tables, beds/nightstands and countertop/television modules.
4. Required fixtures reserve and connect an interaction approach to the
   circulation spine. A required fixture that cannot reserve an approach fails
   the layout; optional props are skipped.
5. `prune_unreachable` rebuilds the furnished walkable mask and verifies every
   door approach, required fixture approach and room route. If optional clutter
   closes a required route, optional props in the affected room are removed and
   the full check reruns. Required-only failures return `FitError` with the
   parcel/lot diagnostics supplied by the planner.

WP 3.3+ can extend the shared rules and strategies without changing the
materializer. Grocery aisle strategies can use `AisleRun`; multi-floor houses
must extend the circulation reservation to stair links and landing cells.

## Supporting changes

- `RoomKind` gained `Bathroom` and `Hall`. Halls intentionally have no furniture
  grammar and remain clear circulation.
- `PlannedSign` now carries a stable placement key, prop ID, attachment layer,
  local offset, facing and optional `ReadableId`.
- Blueprint signs project through `signs_world` into the existing `PropAnchor`
  registry. Readables are synced after props so the wall/floor anchor exists
  before the `Read` prompt can reference it.
- `add_prop_fixture` accepts `WallMount` anchors on wall, window or pillar
  tiles. Floor and countertop fixtures are still validated as before.
- `CellAttachment` now derives `Hash` for the furnishing occupancy set.
- The placeholder readable ID `CraftsmanNote` (`= 5`) has authoritative title
  and body text in `action/readables.rs`.

## Verification record

Commands run from the repository root:

```text
cargo test -p zombiebox-core --lib world::archetypes::houses
cargo test -p zombiebox-core --lib world::furnish
cargo test -p zombiebox-core --lib action::county::tests::craftsman
cargo test -p zombiebox-core --lib action::tests::craftsman
cargo test -p zombiebox-core --lib
cargo test --workspace --no-fail-fast
cargo clippy -p zombiebox-core --lib --tests
```

Results:

- Craftsman arrangement A validates for all four frontage orientations, is
  stable across repeated builds, and fits a representative lot.
- Required fixtures, the searchable fridge and the readable note are present.
- Forward/reverse chunk materialization produces identical cells, building
  owners and prop anchors.
- Every authored door passes the runtime bilateral `0.23 m` capsule sweep,
  closed and open, on both chunk-visitation orders.
- The fridge opens with `terrain_version` unchanged (no static chunk rebuild),
  reaches `PropState::Opened` through `prop_views`, and survives teleport
  away/back with exactly one anchor and the opened state intact.
- The Craftsman note surfaces `InteractPrompt::Read`, opens the authoritative
  text, and a rocket-breach exterior wall remains rubble after revisit.
- `cargo test -p zombiebox-core --lib`: 90 passed.
- `cargo test --workspace --no-fail-fast` passes every target except the
  documented pre-existing baseline failure
  `spatial_kinematics::county_seed_42_terrain_seam_and_diagonal_raycasts`
  (`oblique ray z mismatch: hit_z=5.28868 expected=4.1443267`).
- Clippy reports no new warnings in the WP 3.2 files.

## Open findings / downstream notes

- Street-level and isometric screenshots were not captured in this host-only
  pass. WP 3.2 still needs the furnished-scene and handset review evidence
  listed in the plan before final release sign-off.
- The house and porch share the derived roof slab. A distinct porch roof
  silhouette is WP 3.3 work.
- The fixture is a single floor. Aligned stairs, two-floor support and
  per-wing cutaways are WP 3.3 work.
- The bathroom uses bathtub-relative toilet/sink offsets to keep the 5 × 3
  room's corridor open. A general room-fit solver in WP 3.4 should absorb this
  arrangement and provide a compact fallback for unusual lots.
- The county association between blueprint structures and the legacy
  `Sites`/survey payload is still WP 3.11 work; the runtime uses
  `ChunkSites::structures` plus the shared prop registry.
