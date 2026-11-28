# Phase 3 WP 3.3 handoff — reference residential street

Status: host integration verified; visual and device review remain open.

`World::with_fixture(seed, CountyFixture::ResidentialStreet)` selects two
compatible existing residential lots along the same frontage in deterministic
nearest-to-spawn order. `World::new(seed)` continues to select the single
Craftsman; `World::legacy(seed)` keeps the legacy baseline. `SupportLot` is the
independent two-floor fixture. `World::fixture_plans()` exposes all selected
parcel plans, while `fixture_plan()` remains the first plan.

The pair is selected before relief and generated through normal parcel pads,
chunk streaming and the blueprint materializer. Candidate pairs have matching
frontage direction and boundary line and a gap of at most six tiles. Both
planners must fit before either lot is replaced. Pedestrian paths and
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
Use `--focus ranch` or `--focus craftsman` for a closer facade.

For seed `42`, Craftsman A front door is `(284,195)` and ranch A front
entry is `(313,195)`; both face south. The static car anchor is `(322,190)`,
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

The seed 63 orbit overview is the clearer reference comparison. Seed 42’s
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

The viewer starts across the road from the selected pair. Substitute `--seed
63` to see the other reference arrangement. Use `--support-lot --seed 42` in
place of `--residential-street` for the two-floor support fixture. The normal
viewer still starts the default county when no fixture flag is supplied.

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
