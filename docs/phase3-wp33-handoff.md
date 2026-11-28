# Phase 3 WP 3.3 handoff — reference residential street

Status: host integration verified; visual and device review remain open.

`World::new(seed)` and `CountyFixture::ResidentialStreet` place a Craftsman,
a Ranch, and a Colonial near the county start. Generation keeps three lots
along the starting street wide and deep enough to fit the furnished
footprints. The selector ranks reachable frontages by road distance and
commits the three plans together on distinct lots before relief.
`World::fixture_plans()` exposes all three plans, and `fixture_plan()`
identifies the Craftsman. `World::legacy(seed)` keeps the legacy baseline.
`SupportLot` is the independent two-floor fixture.

The houses use the normal parcel pads, chunk streaming and blueprint
materializer. Pedestrian paths and
vehicle driveways are separate `OutdoorLayer` fields, rendered as gravel and
pavement respectively. `vehicle_slots` use the world-space minimum corner of
an oriented car footprint; the runtime registers the parked car through the
prop anchor registry in its owning chunk. The car is a static prop, not a
drivable vehicle. The driveway is three tiles wide; the garage gate is one
cell wide because the existing bilateral door validator requires solid jambs
on either side. A car-width traversable gate needs a later wide-opening
primitive.

The headless full-presentation preview, including buildings and props, is:

```sh
XDG_RUNTIME_DIR=/tmp VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
  cargo run -p zombiebox-render3d --example preview3d --features gpu -- \
  --scene residential-street --seed 42 --mode iso --size 1280x720
```

Use `--mode orbit` for the alternate view. The output defaults to
`/tmp/zombiebox-preview3d/residential-street-s42-overview-Isometric.png`.
Use `--focus ranch`, `--focus craftsman` or `--focus colonial` for a closer facade.

In the original WP 3.3 captures, seed `42` had Craftsman A at `(284,195)` and ranch A at
`(313,195)`, both facing south. The static car anchor was `(322,190)`,
facing south. The overview player position is `(298,203)` and ranch focus is
`(318,201)`. Seed `63` selects Craftsman B at `(361,341)` and ranch A at
`(322,341)`, facing south, with parked car at `(331,336)` and overview focus
`(341,349)`.

Final full-presentation captures and cold software Vulkan measurements at
1280 × 720 (host render times do not represent handset frame pacing):

| Capture | File | Drawn / resident chunks | Instances | Triangles | Cold render |
| --- | --- | ---: | ---: | ---: | ---: |
| Seed 42 orbit overview | `/tmp/zombiebox-preview3d/residential-street-s42-overview-Orbit.png` | 23 / 25 | 42 | 124,591 | 1.28 s |
| Seed 63 orbit overview | `/tmp/zombiebox-preview3d/residential-street-s63-overview-Orbit.png` | 23 / 25 | 42 | 101,458 | 1.19 s |
| Seed 42 ranch orbit | `/tmp/zombiebox-preview3d/residential-street-s42-ranch-Orbit.png` | 11 / 25 | 50 | 76,499 | 0.96 s |
| Seed 42 isometric | `/tmp/zombiebox-preview3d/residential-street-s42-overview-Isometric.png` | 21 / 25 | 42 | 121,059 | 1.48 s |

In that capture set, the seed 63 orbit overview is the clearer comparison. Seed 42’s
isometric view crops a wing; its ranch orbit view shows both entry and garage
approaches. The static car is inside the roofed garage.
Close street-level recognition and device review remain open.

`cargo test -p zombiebox-core --lib reference_street_is_opt_in_and_deterministic`
passes for seeds 0, 1, 2, 7, 42, 63 and `u64::MAX`, including replay and the
default/legacy selectors. `cargo test -p zombiebox-core --lib reference_street` passed all four tests: replay, both chunk orders and bilateral door sweeps, live pedestrian paths, and both entries with the parked car. `cargo test -p zombiebox-core --lib support_fixture_streams_stairs_wings_and_collapses_after_breach` passed; `cargo test -p zombiebox-core --lib frontage_placement_rejects_insufficient_depth_in_all_orientations` passed.

`cargo test -p zombiebox-core --lib ranch_garage_connection_is_walkable_in_live_action` verifies the live garage connection. `cargo test -p zombiebox-render3d --test support_fixture` verifies that entering the fixture wing controls its roof cutaway.

Final workspace verification:

```sh
XDG_RUNTIME_DIR=/tmp VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
  cargo test --workspace --no-fail-fast
```

Result: 262 passed, zero failed, one ignored diagnostic. The isolated
seed 42 terrain seam and diagonal raycast baseline also passed at pre-change
commit `c2f4de9`.

## Interactive desktop playtest

```sh
cargo run -p zombiebox-render3d --example viewer --features viewer -- --residential-street --seed 42
```

The viewer starts at the normal county spawn. Substitute `--seed 63` to see
another starting neighborhood. Use `--support-lot --seed 42` in place of
`--residential-street` for the two-floor support fixture.

Move with WASD, aim with the arrow keys, fire with Space, and interact with E.
Z/X/C/V select weapons; R reloads, H heals, Shift walks quietly, and P pauses.
Keys 1–5 select cameras; drag to orbit and use the mouse wheel to zoom. B
switches to Birch Street; N returns to the selected county fixture (or the
normal county if no fixture was selected). Esc quits.

The viewer was checked with `cargo check -p zombiebox-render3d --example
viewer --features viewer`, its `--help` output, and `cargo test -p
zombiebox-render3d --example viewer --features viewer
fixture_scenes_start_at_their_access_points`, which verifies both fixture
spawns for seed 42 without opening a window. Interactive desktop play remains
to be exercised on a display.

## WP 3.4 room fitting and Colonial

- Put frontage sizing, seeded arrangement order, center-biased bay cuts and shared-wall opening checks in `world/floorplan.rs`. Keep the orientation basis and setbacks in the existing lot transform and placement path.
- Use the frontage fit in `houses.rs` and `houses/ranch.rs`. The Craftsman can fall back to its compact plan, and its side-hall façade has an independently seeded window pair. The Ranch uses the narrower garage when the wider wing exceeds the frontage. Both keep their public approach and the Ranch has a separate driveway.
- Build the Colonial in `houses/colonial.rs` using `BlueprintGrid`, `RoomMeta`, `DoorIntent` and `StairLink`. Try the seeded A/B order against a 17 × 17 footprint before the 15 × 15 fallback. Arrangement A uses a straight center hall. Arrangement B uses a shorter hall and side stair. Both have a centered brick façade, living and dining areas, a rear kitchen and a supported second floor with an upstairs hall, two bedrooms and a bath.
- Use `furnish.rs` to reserve approach and circulation tiles, put a table and seating in the dining room, and scope furniture dependencies and placement IDs to their authored rooms. In `action/county.rs` and the spatial adapter, rotate both authored stair landings into world space so the stairs and railings share the same endpoints. Use the existing materializer for the fixture.

Launch the opt-in Colonial playtest with:

```sh
cargo run -p zombiebox-render3d --example viewer --features viewer -- --colonial-lot --seed 42
```

Capture the facade with `preview3d --scene colonial --seed 42 --mode orbit`.
Use `--focus upstairs` to inspect the stair hall or `--focus bedroom` for a furnished cutaway. Seed `42` selects Colonial B with north frontage. Seed `0` selects Colonial A with north frontage. Seed `18446744073709551615` selects Colonial A with west frontage for the live orientation check.

Seed `42` now starts at `(354,163)`. The north-facing front doors are
`(354,169)` for the Craftsman, `(317,170)` for the Ranch, and `(284,167)`
for the Colonial. The audit over seeds `0..63` and `u64::MAX` found all
three houses, with the longest walk to a front door at 81 tiles. Live
input tests walked from spawn through all three doors in seeds `42` and
`u64::MAX`. Starter placement favors a clear departure from the shoulder,
and all 17 motorcycle tests passed. The starter Axe is on the Craftsman
approach, and the pistol with 12 rounds is inside its front door.

The Colonial tests cover eight seeds over all four frontage orientations and three lot sizes. The live checks walk from the road to both stair layouts and back down, sweep both stories’ doors in both directions, replay opposite chunk orders, and compare frontage placement after changing loot profiles. The workspace suite passed 271 tests with one ignored diagnostic. The viewer fixture-start test also passed.

The 1280 × 720 software Vulkan captures are in `/tmp/zombiebox-phase3`. The seed 42 Colonial facade drew 17 chunks and 69,479 triangles. The seed 0 bedroom cutaway drew 19 chunks and 110,455 triangles. The refreshed seed 42 street overview includes both approaches, and the seed 63 isometric view shows the SideHall Craftsman next to the Ranch. Interactive display play and physical Android frame pacing remain open.
