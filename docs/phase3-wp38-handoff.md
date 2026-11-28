# Phase 3 · WP 3.8 handoff

The default county and field atlas use generator v3. The seed still identifies a reproducible 512 × 512 map. Version 3 changes the map, placements, and coordinates for existing seeds. The legacy constructor and individual building review fixtures retain their v2 geography and noise streams.

## Generation contracts

- Geography paints the watershed, lake, mountain ridge, and land cover before the planner reads the natural field. `NaturalTerrainSample` gives tile-center elevation in tile units, rise per tile, Chebyshev distance from water in tiles, and suitability from 0 to 1.
- `World::town_plan()` exposes the highway ribbon, collector, and local street. Roads are ordered by class priority. The continuous centerlines use world tile-center coordinates and share samples at junctions. `road_at` and `raster` identify each tile's class via its road index and give the owning centerline segment. Regional approaches and civic, downtown, military, and woodland parcels use the county geography adapter.
- The planner scores bounded smooth arterial candidates against the natural field, checks shoulder clearance, and rejects radii under 18 tiles. A seeded tie-break resolves equal scores. Collector and local branches select dry corridors and end the local street with a paved 5 × 5 turnaround.
- Road elevations are solved along centerlines and projected across the road width. The shared corner grid joins roads, terrain triangles, and flat building foundations. Ribbon road skirts blend back into the natural terrain across a 12-tile band. Existing waterbed and bridge-deck sampling remains in use.
- `TownPlan::frontages` provides cardinal block reserves for WP 3.9. Each `FrontageSlot` carries an inclusive rectangle, frontage side, stable ID, road class, use hint, and a tile-to-tile connector. Reserves are separate from the existing county lots. A connector spans one to four tiles from the road tile to the frontage midpoint. The suggested pad elevation blends natural height with the graded road and caps the road-to-pad endpoint rise at 12% of the connector length.
- `road_hash()` fingerprints the classified road raster. `canonical_hash()` covers the centerlines, route anchors, classified raster, and reserved frontages. Chunk visits and building-content streams do not participate.

## WP 3.9 and WP 3.10

- WP 3.9 can subdivide the reserves along `usable_interval()`. That interval describes the frontage edge. Revalidate a dry connector and the final grade for each child lot and pass the pad, apron, driveway, and parking into terrain generation and dressing. Commercial reserves are 28 × 28 tiles and residential reserves are 16 × 18. Reserve suitable retail sites before competing civic placements when their availability must be guaranteed. The current foundation setter operates per parcel. For the gas station's mart and canopy, solve one shared pad or assign the solved heights to individual structures so both align with the terrain.
- WP 3.10 can reuse the natural terrain sampler, classified segment raster, and road profile projection. The ribbon grades each branch from its first centerline sample, which it shares with an earlier road, and carries a single turnaround. Introduce junction nodes and per-road endpoint and turnaround ownership when a morphology has multiple junctions, dead ends, or loops. Pass validated bridge crossings and their fixed deck grades to the terrain solve. The current town planner uses the dry valley shelf and the county adapter carries the regional river crossing.

## Verification

The baseline at `eeba1a8` passed the seed 42 terrain seam and diagonal raycast check and all seven spatial relief checks. Its workspace run stopped on three garage-door failures. The two bilateral door sweeps expect an ordinary door at a garage door's middle segment, and the Ranch interaction check fails to open all three garage segments.

The ribbon introduced a steep transition on seed 7 when the hospital pad overwrote a shared road corner. The geography adapter now reserves a tile between civic pads and the road raster to keep both grades intact.

Use these commands to repeat the host checks.

```sh
cargo test -p zombiebox-core --no-fail-fast --test county --test spatial_kinematics --test spatial_relief --test motorcycle --test highway_ribbon
cargo test -p zombiebox-core --release --test county audit_ribbon_geography_across_seeds -- --ignored
cargo test -p zombiebox-core --lib reference_street_tests
cargo test --workspace --no-fail-fast
sh scripts/check-native.sh
```
