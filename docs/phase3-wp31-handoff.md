# Phase 3 WP 3.1 handoff — blueprint-to-county

Status: implemented and verified on host tests.

> Superseded default: WP 3.2 now makes `CountyFixture::CraftsmanLot` the
> `World::new` default. This handoff still describes the smaller
> `CountyFixture::BlueprintLot` contract fixture, which remains available
> explicitly. See [WP 3.2 handoff](phase3-wp32-handoff.md).

## Fixture selector

The normal county entry points now use the blueprint fixture by default, per an
early testing request:

```rust
use zombiebox_core::{CountyFixture, World};

let world = World::new(seed);                    // BlueprintLot
let legacy = World::legacy(seed);                // pre-Phase-3 baseline
let explicit = World::with_fixture(seed, CountyFixture::BlueprintLot);
```

- `World::new(seed)` is now equivalent to
  `World::with_fixture(seed, CountyFixture::BlueprintLot)`.
- `World::legacy(seed)` retains `CountyFixture::None` for regression comparisons.
- `World::fixture()` reports the active selector.
- `World::fixture_plan() -> Option<&ParcelPlan>` exposes the authored parcel for
  previews and tests.
- The fixture replaces one existing residential/city/woods lot. It prefers a
  lot near the residential spawn whose even-span prop can cross a chunk seam.
  Seeds `0`, `1`, `2`, `42`, and `u64::MAX` were inspected; seed `u64::MAX`
  places the fixture outside the initial streaming window, which is used by the
  streaming-order test.

## Names and entry points

| Role | Name |
| --- | --- |
| Parcel dispatch | `world::Parcels::new` → `select_fixture_parcel` → `archetypes::FixtureParcel` (`CompoundPlanner::plan`) |
| Parcel plan | `world::blueprint::ParcelPlan` |
| Structure | `world::blueprint::PlacedStructure` |
| Blueprint | `world::blueprint::BlueprintGrid` + `BlueprintCell` |
| Placement search | `world::blueprint::place_blueprint_in_lot` |
| Validation | `BlueprintGrid::validate` → `FitError` |
| Local transform | `world::blueprint::LotTransform` |
| Runtime registration | `CountyStage::register_blueprint_building` |
| Runtime materialization | `CountyStage::materialize_chunk` (legacy cells, then blueprint cells) |
| Runtime prop handoff | `CountyStage::blueprint_prop_anchors` → `ActionGame::sync_county_blueprint_props` |
| Prop registry access | `ActionGame::prop_anchors` / existing `PropAnchor` path |

The legacy site path is unchanged for all non-fixture parcels. `ChunkSites` now
also carries `structures: Vec<&PlacedStructure>`; only the fixture parcel fills
it in this package.

## Coordinate contract

- Local `u` runs along the frontage. Local `v` runs into the lot, so `v = 0` is
  the frontage wall for every orientation.
- `LotTransform::local_to_world` is `origin + u * right + v * inward`.
- Frontage bases:
  - North: `right = (-1, 0)`, `inward = (0, 1)`
  - South: `right = (1, 0)`, `inward = (0, -1)`
  - West: `right = (0, 1)`, `inward = (1, 0)`
  - East: `right = (0, -1)`, `inward = (-1, 0)`
- Door tangents, stair vectors, prop facings and prop offsets use the same
  basis. A local `DoorAxis::AlongX` follows `right`; `AlongY` follows `inward`.
- Multi-tile props transform every covered local cell, take the world min
  corner and world span, then feed the existing `PropAnchor::pivot` path. The
  authored rotation is applied once.

### Even-span coordinate example

The fixture authors `PropId::Table` (`2 × 1`) at local `(3, 2, 0)`.

| Frontage | World min corner | World span | World facing | Pivot offset |
| --- | --- | --- | --- | --- |
| North/South | transformed local min | `2 × 1` | South/North | `(+0.5, 0, 0)` |
| West/East | transformed local min | `1 × 2` | West/East | `(0, +0.5, 0)` |

The fixture placement search deliberately aligns this table across a chunk seam.
The county test compares both chunk visit orders and verifies the same stable
`anchor_uid`, tile, span and owner.

## IDs and ownership

- `ParcelId`, `StructureId`, `LocalPlacementKey` and `PlacementId` are stable
  seeded mixes of the world-space parcel rectangle, structure slot and authored
  local key.
- `PlacedStructure::id` becomes `BuildingInfo::uid` (`BuildingUid`). Runtime
  `BuildingId` remains a transient slot; tests compare UIDs.
- A blueprint prop's `PlacementId` is its `PropAnchor::anchor_uid`. Re-running
  streaming is idempotent, and revisiting a parcel leaves exactly one instance.
- Each blueprint cell carries explicit `building_id`, `surface`, `indoor`,
  `load_bearing`, `door_axis`, `stair_dir` and material. Roof and floor slabs
  continue to be derived from `BuildingInfo` ownership in the spatial adapter.

## Verification record

Commands run from the repository root:

```text
cargo test -p zombiebox-core --lib world::blueprint
cargo test -p zombiebox-core --lib world::archetypes
cargo test -p zombiebox-core --lib action::county::tests::blueprint
cargo test -p zombiebox-core --lib
cargo test --workspace
```

Results:

- Blueprint transform tests pass for all four frontage orientations, including
  door tangent/axis, asymmetric footprint, even-span prop footprint and pivot.
- Validation rejects a blocked door approach and unsupported upper-floor cells.
- Adding optional clutter leaves the fixture structure, openings, stairs and
  required placement ID unchanged.
- The county fixture test passes with chunks streamed in forward and reverse
  order; owner UID, footprint, runtime cells and prop placement match.
- The fixture door passes the runtime bilateral `0.23 m` capsule sweep, closed
  and open.
- The fixture roof is owned by the registered blueprint structure, and an
  `ActionGame` teleport away/back reports exactly one prop instance.
- `World::new` / `ActionGame::county` stream the fixture by default, while
  `World::legacy` remains available for comparisons.
- `cargo test -p zombiebox-core --lib`: 81 passed.
- `cargo test --workspace --no-fail-fast` passes every target except the documented
  pre-existing baseline failure `spatial_kinematics::county_seed_42_terrain_seam_and_diagonal_raycasts`
  (`oblique ray z mismatch: hit_z=5.28868 expected=4.1443267`). The same failure
  was reproduced from a clean worktree at the starting commit `abec79b`.

## Open findings / downstream notes

- Phase 3.1 intentionally carries only fixture-scale data. `PlacementStrategy`,
  room-fit budgets, multi-structure parcel pads, clothing/outdoor path
  consumption and sign materialization remain for WPs 3.2–3.9.
- `BlueprintGrid::validate` already enforces door stencils and vertical support;
  downstream room fitting should extend it rather than bypass it.
- The fixture parcel currently has no legacy `Sites` building/loot payload. The
  runtime uses `ChunkSites::structures`; later packages that need survey/atlas
  output for authored structures should add a common projection instead of
  duplicating the fixture into `Sites`.
- The full Phase 3.11 cutover is still open: remaining legacy districts are
  still generated, and `GENERATOR_VERSION` / save-delta rejection have not been
  bumped yet. The default switch here is intended to expose the new structure
  path for early gameplay review.
