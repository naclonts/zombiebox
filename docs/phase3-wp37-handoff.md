# Phase 3 WP 3.7 handoff

## MVP design

- `world/archetypes/retail.rs` fits a frontage-local envelope in `GasStationParcel::plan`, then offsets the mart and canopy from the same `LotTransform`. The `ParcelPlan` carries two `PlacedStructure` IDs and the outdoor walking, driving, and parking layout. `gas_mart_blueprint` and `gas_canopy_blueprint` own their cells and stable fixture keys.
- Reserve the drive-through and public approach before placing the pumps, pylon, and parked cars. Transform occupied car tiles before computing each world-space anchor so rotated parking keeps the intended clearance. The canopy uses outdoor floor cells and load-bearing pillars with the shared roof implementation.
- `world/parcels.rs` selects a compatible road-front lot for `CountyFixture::GasStationLot`. The County materializer registers the structures and fixtures through the normal blueprint and chunk paths. The price pylon uses the Read interaction. The fixture stages a drivable motorcycle through the existing vehicle registry.
- The shared prop catalog supplies gameplay bounds for the pump and pylon. The renderer uses simple kit meshes for both. The mart uses the grocery’s shelf, checkout lane, and till kit.

## Review

```sh
cargo run -p zombiebox-render3d --features viewer --example viewer -- --gas-station-lot --seed 42
cargo run -p zombiebox-render3d --features gpu --example preview3d -- --scene gas-station --seed 42 --focus frontage --mode orbit --size 1280x720
cargo run -p zombiebox-render3d --features gpu --example preview3d -- --scene gas-station --seed 42 --focus interior --mode iso --size 1280x720
```

The viewer starts at the road access. Use `E` to interact or mount. While mounted, `W` throttles, `S` brakes or pushes in reverse, `A` and `D` steer, and the left and right arrows lean. Reading the pylon prints its text in the terminal.

Seed `42` has north frontage and uses the inclusive lot `(305, 236)` through `(332, 264)`. The public route starts at `(319, 236)` and enters the mart at `(318, 251)`. Seed `63` has south frontage and uses `(313, 281)` through `(340, 309)`.

## WP 3.9 lot inputs

Budget at least **28 frontage tiles × 28 depth tiles** for the authored compound. The current fitter reserves a 24 × 24 local envelope with a two-tile setback on every side. Link a dry road approach to `Frontage::access` on the boundary and leave the frontage-to-apron connection clear of dressing. The 19 × 12 forecourt includes the roofed pump lane. Two parked-car footprints sit beside the mart, clear of the lane and the public path. Use the existing `LotTransform` to rotate the envelope, fixtures, and approach together.

## Host verification

- `cargo test --workspace gas_ -- --nocapture` passes eight focused checks. These cover deterministic fits on seeds `0`, `1`, `7`, `42`, and `63` plus synthetic lots in all four orientations. Seed `42` exercises the open-air canopy roof owner, pylon Read, stable structures, cars, and pump identities on streaming revisit, and a walk from the frontage through the till interaction to the public WC.
- The seed `42` motorcycle test starts on the road at `(320, 235)`, drives between the pumps to about `(320, 246)`, then reverses back onto the road with 100% bike condition. The parked cars stay in place throughout.
- `cargo test -p zombiebox-render3d --lib gas_canopy_keeps` verifies the roofed driveway keeps its paved surface in the terrain mesh. `cargo test -p zombiebox-render3d --features viewer --example viewer` verifies fixture deployment from the road access. Headless GPU captures cover the seed `42` frontage and interior and seed `63` frontage.
- `cargo test --workspace` passed with one existing ignored test. The seed `42` terrain seam and diagonal raycast test and the bilateral door and gate capsule sweep both passed.

The fixture uses prototype pump and pylon meshes and a reused fridge as a cooler. A forward turnaround, manual browser play, and physical Android frame pacing remain open for later integration and review.
