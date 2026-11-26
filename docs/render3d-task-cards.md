# Zombiebox 3D — Agent Task Cards (Desktop-First)

**Workflow:** You (the coordinator) spin up one fresh agent session per task card below, review the diff and test output, update `docs/render3d-progress.md`, and merge before starting the next dependent card.

### Tier Legend
* **`[Lightweight OK]`** — Narrow write set, pure Rust / deterministic unit tests, safe for GLM 5.3 flash.
* **`[Coordinator / Frontier]`** — Involves unsafe FFI/JNI, `wgpu` 26 API setup, or complex cross-file architecture. Run with a stronger model or review closely.

---

## Reusable Session Prompt Template

Copy-paste this into each agent session, filling in `<TASK_ID>`:

```text
Hola! Implement task I <TASK_ID> from docs/render3d-task-cards.md.
Read:
1. AGENTS.md
2. docs/render3d-architecture.md
3. docs/render3d-progress.md
4. The task ID card in docs/render3d-task-cards.md
5. The files explicitly listed in the card's Read Set and Write Set.

Rules:
- Stay strictly inside the Write Set. Do not refactor unrelated files.
- Follow the exact struct/trait names and coordinate conventions in docs/render3d-architecture.md.
- Before editing, output a brief 3-5 bullet plan.
- After editing, run EVERY command in the card's Verification section and report the exact output. Never claim unrun tests passed.
- If a contract conflicts with existing code, stop and report the exact compiler/API conflict rather than inventing a workaround.
- Update docs/render3d-progress.md with status, tests run, and any notes.
```

---

## Phase 0: Baseline & Desktop GPU Feasibility

### `T00a` — Baseline, `wgpu` Pin & Desktop Headless Smoke
* **Tier:** `[Coordinator / Frontier]`
* **Depends on:** None
* **Read Set:** `Cargo.toml`, `crates/zombiebox-render/examples/preview.rs`, `scripts/check-native.sh`, `AGENTS.md`
* **Write Set:** `Cargo.toml`, `Cargo.lock`, `crates/zombiebox-render3d/Cargo.toml`, `crates/zombiebox-render3d/src/lib.rs`, `crates/zombiebox-render3d/examples/smoke_triangle.rs`, `docs/render3d-progress.md`
* **Goal:**
  1. Capture baseline CPU reference screenshots (Birch Street & County seed 42) using `zombiebox-render`'s `preview` example; record exact CLI flags and dimensions in `docs/render3d-progress.md`.
  2. Scaffold `crates/zombiebox-render3d` with optional `gpu` (`wgpu = "26.0.1"`, `bytemuck`, `pollster`) and `viewer` (`winit`) features, plus `glam` as default dep. Verify `Cargo.lock` builds cleanly on Rust `1.87.0`. (If `wgpu 26.0.1` fails MSRV/transitives, pick the closest verified version and record it in the ledger.)
  3. Build `examples/smoke_triangle.rs` that renders a depth-tested colored triangle offscreen and writes a PNG. Record desktop adapter/backend in ledger.
* **Verification:**
  ```sh
  cargo +1.87.0 test --workspace --locked
  cargo +1.87.0 check -p zombiebox-render3d --no-default-features --locked
  cargo +1.87.0 run -p zombiebox-render3d --example smoke_triangle --features gpu --locked
  ```
* *(Note: Android `ANativeWindow` surface spike is intentionally deferred to `T10b` so desktop e2e progress is unblocked.)*

---

## Phase 1: Authoritative 3D Spatial Core (`zombiebox-core`)

### `S01a` — Spatial Primitives & Terrain Grid Sampler
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T00a`
* **Read Set:** `crates/zombiebox-core/src/lib.rs`, `crates/zombiebox-core/src/world.rs`
* **Write Set:** `crates/zombiebox-core/src/spatial/mod.rs`, `crates/zombiebox-core/src/spatial/terrain.rs`, `crates/zombiebox-core/src/lib.rs`, `crates/zombiebox-core/tests/spatial_terrain.rs`
* **Goal:**
  1. Implement `FLOOR_H`, `Vec3`, `Aabb3`, `BodyShape`, `SupportId`, `MotionState`, `BodyState` in `spatial/mod.rs`.
  2. Implement `TerrainGrid` ($513 \times 513$ quantized `i16` at $1/256$ units), `TerrainCell`, `TerrainSample`, and flat-zero builder for Birch Street.
  3. Implement `TerrainGrid::cell(Position)` and `TerrainGrid::sample(Vec2)` using the exact `00 -> 11` diagonal triangle split and outward `+z` normal. Return `None` out of bounds (never clamp).
  4. Write unit tests verifying triangle continuity across cell edges, exact diagonal split, normal orientation, quantization round-trip, and OOB `None`.
* **Verification:**
  ```sh
  cargo test -p zombiebox-core --test spatial_terrain --locked
  ```

### `S01b` — Deterministic Relief, Building Pads & Road Grading
* **Tier:** `[Coordinator / Frontier]` *(algorithmic density: road grade relaxation)*
* **Depends on:** `S01a`
* **Read Set:** `crates/zombiebox-core/src/world.rs`, `crates/zombiebox-core/src/world/geography.rs`, `crates/zombiebox-core/src/world/parcels.rs`, `crates/zombiebox-core/src/world/sites.rs`
* **Write Set:** `crates/zombiebox-core/src/world/relief.rs`, `crates/zombiebox-core/src/world.rs`, `crates/zombiebox-core/src/world/geography.rs`, `crates/zombiebox-core/tests/spatial_terrain.rs`
* **Goal:**
  1. Convert `Tile.elevation` (`u8`) into global continuous heights ($1/16$ unit/byte + deterministic seeded variation + 3-pass quantized smoothing).
  2. Compute order-independent flat building pads (`foundation_z`) from stable parcel/site descriptors and blend surrounding 2–4 tiles.
  3. Relax road-graph heights in sorted deterministic order to enforce max grade $\le 0.20$ on required roads, with bounded deterministic repair if anchors conflict.
  4. Add 64-seed integration test: nonzero relief on seed 42, at least one sloped traversable road, flat pads, road grade compliance, and order-independent chunk queries.
* **Verification:**
  ```sh
  cargo test -p zombiebox-core --test spatial_terrain --locked
  ```

### `S01c` — Hydrology, Bridge Decks & `World` API Wiring
* **Tier:** `[Lightweight OK]`
* **Depends on:** `S01b`
* **Read Set:** `crates/zombiebox-core/src/world/relief.rs`, `crates/zombiebox-core/src/world.rs`
* **Write Set:** `crates/zombiebox-core/src/world/relief.rs`, `crates/zombiebox-core/src/world.rs`, `crates/zombiebox-core/tests/spatial_terrain.rs`
* **Goal:**
  1. Compute constant water planes per connected lake and non-increasing downstream river profiles; ensure riverbed $\ge 0.25$ below water.
  2. Compute elevated bridge deck corners ($\ge 0.25$ above water) with matching graded road approach edges.
  3. Expose `World::terrain_at`, `World::terrain_cell`, `World::elevation_at`, and `World::foundation_z`.
  4. Freeze deterministic hash test across seeds and verify cross-seam corner equality.
* **Verification:**
  ```sh
  cargo test -p zombiebox-core --locked
  ```

### `S02a` — Spatial Support/Collision Contracts & Synthetic Test Fixtures (TDD)
* **Tier:** `[Lightweight OK]`
* **Depends on:** `S01c`
* **Read Set:** `crates/zombiebox-core/src/spatial/mod.rs`, `crates/zombiebox-core/src/spatial/terrain.rs`
* **Write Set:** `crates/zombiebox-core/src/spatial/support.rs`, `crates/zombiebox-core/src/spatial/collision.rs`, `crates/zombiebox-core/tests/spatial_collision.rs`
* **Goal:**
  1. Define `SupportSurface`, `ColliderId`, `ColliderShape`, `Collider`, `Ray3`, `Hit3`, `MoveResult`, `SpatialWorld`, and `move_kinematic` signature (with `todo!()` bodies or minimal stubs).
  2. Build a `MockSpatialWorld` in `tests/spatial_collision.rs` with deterministic unit tests covering:
     * Flat ground + sloped ramp climb/descent (reject slope $> 0.7$).
     * Bridge underpass (body under bridge must NOT snap to deck).
     * Thin-wall high-speed sweep (no tunneling).
     * Stair ramp continuous ascent/descent and landing threshold.
     * Support removal -> `Airborne` + gravity fall -> landing on lower slab.
     * Ceiling block when rising.
     * Raycast & sphere-cast (for camera obstruction) hitting intact box/terrain.
* **Verification:**
  ```sh
  cargo test -p zombiebox-core --test spatial_collision --no-run --locked
  ```

### `S02b` — Support Resolver, Raycast & Sphere-Cast Implementation
* **Tier:** `[Lightweight OK]`
* **Depends on:** `S02a`
* **Read Set:** `crates/zombiebox-core/src/spatial/support.rs`, `crates/zombiebox-core/src/spatial/collision.rs`, `crates/zombiebox-core/tests/spatial_collision.rs`
* **Write Set:** `crates/zombiebox-core/src/spatial/support.rs`, `crates/zombiebox-core/src/spatial/collision.rs`
* **Goal:**
  1. Implement reachable support selection (nearest foot `z` within step-up `0.2` / step-down `0.3` bounds, stable tie-break by `SupportId`).
  2. Implement `raycast` (slab AABB + terrain/deck/stair triangle intersection, smallest non-negative `t`, stable tie-break by `ColliderId`).
  3. Implement `sphere_cast` (expanded AABB + terrain margin) for camera obstruction.
* **Verification:**
  ```sh
  cargo test -p zombiebox-core --test spatial_collision ray sphere support --locked
  ```

### `S02c` — Kinematic Solver & Live `ActionGame` Spatial Adapter
* **Tier:** `[Coordinator / Frontier]` *(kinematic sweep + live tile collider mapping)*
* **Depends on:** `S02b`
* **Read Set:** `crates/zombiebox-core/src/action/types.rs`, `crates/zombiebox-core/src/action/block.rs` (wall/door/stair structs only), `crates/zombiebox-core/src/action/county.rs` (bounds/tile lookup only)
* **Write Set:** `crates/zombiebox-core/src/spatial/collision.rs`, `crates/zombiebox-core/src/action/spatial.rs`, `crates/zombiebox-core/src/action/mod.rs`, `crates/zombiebox-core/tests/spatial_collision.rs`
* **Goal:**
  1. Implement `move_kinematic` (swept circle/AABB vs wall boxes up to 3 slide iterations, gravity `4.0` units/s$^2$, step/slope/ceiling checks) so all `S02a` synthetic tests pass.
  2. Define shared core wall-segment geometry helpers in `action/spatial.rs` so physics and future 3D building meshes share exact wall/door/window dimensions.
  3. Implement `SpatialWorld` adapter for `ActionGame` (`ActionGame::spatial_world()`) reading live `TileRenderInfo`, doors, stairs, bridges, and terrain.
  4. Add live Birch & County tests verifying door toggle / wall breach immediately updates collision queries.
* **Verification:**
  ```sh
  cargo test -p zombiebox-core --test spatial_collision --locked
  ```

### `S03a` — Action Types 3D Migration & Compatibility Shims
* **Tier:** `[Lightweight OK]`
* **Depends on:** `S02c`
* **Read Set:** `crates/zombiebox-core/src/action/types.rs`, `crates/zombiebox-core/src/lib.rs`
* **Write Set:** `crates/zombiebox-core/src/action/types.rs`, `crates/zombiebox-core/src/lib.rs`
* **Goal:**
  1. Evolve `Pose`, `MonsterView`, `ProjectileView`, `LightSourceView`, and `ActionEvent` locations to carry authoritative `Vec3` positions (absolute `z` in world units) while retaining semantic `Floor`.
  2. Add temporary compatibility helper methods (e.g. `xy()`) so downstream crates (`zombiebox-render`, `zombiebox-ffi`) continue compiling cleanly during `S03b–S03d`.
* **Verification:**
  ```sh
  cargo check --workspace --locked
  ```

### `S03b` — Migrate Player, Block & County Movement to 3D
* **Tier:** `[Lightweight OK]`
* **Depends on:** `S03a`
* **Read Set:** `crates/zombiebox-core/src/action/mod.rs`, `crates/zombiebox-core/src/action/spatial.rs`
* **Write Set:** `crates/zombiebox-core/src/action/mod.rs`, `crates/zombiebox-core/src/action/block.rs`, `crates/zombiebox-core/src/action/county.rs`, `crates/zombiebox-core/tests/county.rs`
* **Goal:**
  1. Own player `BodyState` in `ActionGame`; route player movement, stairs, vaults, teleports, and floor-collapse falls through `move_kinematic` and `SpatialWorld`.
  2. Preserve exact flat Birch Street movement speed, quiet-walk threshold, and injury-on-landing semantics.
  3. Add integration tests: uphill/downhill `z` change on seed 42, bridge-over-water crossing, stair floor transition, collapsed floor fall.
* **Verification:**
  ```sh
  cargo test -p zombiebox-core --locked
  ```

### `S03c` — Migrate Monster AI & Navigation (`action/ai.rs` isolated)
* **Tier:** `[Lightweight OK]`
* **Depends on:** `S03b`
* **Read Set:** `crates/zombiebox-core/src/action/ai.rs`, `crates/zombiebox-core/src/action/spatial.rs`
* **Write Set:** `crates/zombiebox-core/src/action/ai.rs`
* **Goal:**
  1. Migrate monster bodies to `BodyState` and route movement through `move_kinematic`.
  2. Update grid/semantic-floor A* edge checks to require valid support, walkable grade/step/clearance, and 3D world-distance edge cost.
  3. Update monster LOS checks to use 3D `raycast` with eye/target heights (do not reveal upper/lower floors across slopes).
* **Verification:**
  ```sh
  cargo test -p zombiebox-core --locked
  ```

### `S03d` — Migrate Weapons, Projectiles & Events (`action/weapons.rs` isolated)
* **Tier:** `[Lightweight OK]`
* **Depends on:** `S03c`
* **Read Set:** `crates/zombiebox-core/src/action/weapons.rs`, `crates/zombiebox-core/src/action/spatial.rs`
* **Write Set:** `crates/zombiebox-core/src/action/weapons.rs`, minimal compile fixes in `crates/zombiebox-render/src/*` and `crates/zombiebox-ffi/src/*` if needed
* **Goal:**
  1. Convert weapon origins, targets, projectiles, blasts, and emitted `ActionEvent` locations to authoritative 3D `Vec3`.
  2. Use swept 3D segment / `raycast` queries for bullets/rockets against actors and geometry (earliest hit wins).
  3. Preserve legacy planar aiming behavior on flat Birch Street fixtures.
  4. Ensure entire workspace compiles and all existing tests pass.
* **Verification:**
  ```sh
  cargo test --workspace --locked
  ```

---

## Phase 2: Desktop 3D Engine & Interactive Viewer (`zombiebox-render3d`)

### `T01` — Coordinates & Camera Math (Pure CPU)
* **Tier:** `[Lightweight OK]`
* **Depends on:** `S01a` *(can run in parallel with S01b–S03 if desired)*
* **Read Set:** `docs/render3d-architecture.md` (§2)
* **Write Set:** `crates/zombiebox-render3d/src/coords.rs`, `crates/zombiebox-render3d/src/camera.rs`, `crates/zombiebox-render3d/src/lib.rs`, `crates/zombiebox-render3d/tests/camera.rs`
* **Goal:**
  1. Implement `coords.rs` (`world_point`, `core_point`, `ground_point`, `floor_y`, `tile_corners`).
  2. Implement `CameraMode` (5 modes), `Projection`, `CameraTarget`, `CameraConfig`, `Viewport`, `CameraFrame`, `Ray`, and `CameraRig` with exponential half-life damping and shortest-angle yaw wrap.
  3. Implement `project`, `ray_from_pixel`, `ground_hit`, and `screen_stick_to_world` (exact legacy iso analog mapping + yaw-relative mapping for non-iso modes).
  4. Write unit tests for `+X/+Z/floor` legacy deltas (`+20,+10`, `-20,+10`, `0,-34`), yaw 0/90/180/270, overhead finite matrices, unproject round-trip, analog magnitude, and teleport snap.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --no-default-features --test camera --locked
  ```

### `T02` — Scene Snapshot & Chunk Invalidation Cache (Pure CPU)
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T01`, `S03d`
* **Read Set:** `crates/zombiebox-core/src/action/mod.rs` (public query methods only), `crates/zombiebox-core/src/action/types.rs`
* **Write Set:** `crates/zombiebox-render3d/src/scene/mod.rs`, `crates/zombiebox-render3d/src/scene/snapshot.rs`, `crates/zombiebox-render3d/src/scene/chunks.rs`, `crates/zombiebox-render3d/tests/scene.rs`, optional read-only `render_bounds()` / `render_terrain_version()` in `zombiebox-core`
* **Goal:**
  1. Implement `ChunkKey` (`div_euclid(16)`), `CellKey`, `TileSnapshot`, `FrameSnapshot`, `ChunkSnapshot`, `SceneDelta`, `TileLookup`, `NeighborTiles`, and `SceneState::sync(&ActionGame)`.
  2. Compute deterministic per-chunk geometry fingerprints (no random `HashMap` order) with 1-tile halo propagation on edge changes.
  3. Write tests: negative chunk coordinates, active-window shift, teleport/revisit, seed change reset, zero geometry rebuild on idle/light-only frame, dirty chunk on door toggle/destruction.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --no-default-features --test scene --locked
  ```

### `T03a` — Mesh Primitives, Builder & Winding Tests (Pure CPU)
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T01`
* **Read Set:** `docs/render3d-architecture.md` (§4)
* **Write Set:** `crates/zombiebox-render3d/src/mesh/mod.rs`, `crates/zombiebox-render3d/src/mesh/primitives.rs`, `crates/zombiebox-render3d/src/materials.rs`, `crates/zombiebox-render3d/tests/geometry.rs`
* **Goal:**
  1. Implement `GeometryClass`, `Vertex` (with `Pod`/`Zeroable` & explicit stride/offset assertions), `MeshData`, `MeshLayer`, `ChunkMesh`, `Aabb`, and `MeshBuilder` (`quad`, `cuboid`, `cylinder`, `wedge`, `append_transformed` with inverse-transpose normals).
  2. Implement starter `MaterialRegistry` (sRGB $\to$ linear conversion, stable material IDs).
  3. Write unit tests verifying CCW outward winding on all primitives, normal transforms under non-uniform scale, and index bounds.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --no-default-features --test geometry --locked
  ```

### `T03b` — `wgpu` Pipeline, Low-Res Upscale & Headless Preview
* **Tier:** `[Coordinator / Frontier]` *(wgpu 26.0.1 pipelines, WGSL, 256-byte aligned readback)*
* **Depends on:** `T02`, `T03a`
* **Read Set:** `crates/zombiebox-render3d/examples/smoke_triangle.rs`, `crates/zombiebox-render3d/src/mesh/mod.rs`
* **Write Set:** `crates/zombiebox-render3d/src/gpu/*`, `crates/zombiebox-render3d/src/shaders/world.wgsl`, `crates/zombiebox-render3d/src/shaders/upscale.wgsl`, `crates/zombiebox-render3d/examples/preview.rs`, `crates/zombiebox-render3d/tests/gpu_smoke.rs`
* **Goal:**
  1. Implement `GpuRenderer` (`new_headless`, `new_surface`, `resize`, `render`, `readback_rgba`, `invalidate_scene`, `stats`), opaque depth-tested pipeline (`Depth32Float`, back-face cull), and nearest-neighbor full-screen upscale pass.
  2. Implement chunk buffer residency keyed by geometry fingerprint (zero uploads on camera-only movement).
  3. Write `gpu_smoke.rs`: render flat ground + two overlapping boxes, assert nearer box wins regardless of draw order, verify resize and padded readback.
  4. Wire `examples/preview.rs` CLI (`--scenario`, `--seed`, `--camera`, `--size`, `--output`) to render test geometry from all 5 camera presets.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --features gpu --test gpu_smoke --locked
  cargo run -p zombiebox-render3d --example preview --features gpu --locked -- --scenario birch --seed 42 --camera iso --size 960x540 --output /tmp/zb-t03-iso.png
  ```

### `T04` — 3D Terrain Meshing
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T03b`, `S01c`
* **Read Set:** `crates/zombiebox-core/src/action/types.rs` (`Surface` enum), `crates/zombiebox-render3d/src/mesh/mod.rs`
* **Write Set:** `crates/zombiebox-render3d/src/mesh/terrain.rs`, `crates/zombiebox-render3d/src/mesh/mod.rs`, `crates/zombiebox-render3d/tests/geometry.rs`
* **Goal:**
  1. Exhaustively map all `Surface` variants using authoritative `TerrainCell` corners and the exact `00 -> 11` diagonal split (converted to CCW renderer winding).
  2. Emit road markings/curbs (with anti-z-fighting offset), water planes at authoritative `water_z`, separate bridge decks at `bridge_deck_z`, and zero geometry for `Air`.
  3. Verify zero seam gaps across adjacent chunks on seed 42 (residential, bridge/river, mountain) and render preview PNGs.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --no-default-features --test geometry --locked
  cargo run -p zombiebox-render3d --example preview --features gpu --locked -- --scenario county --seed 42 --camera overhead --size 960x540 --output /tmp/zb-t04-overhead.png
  ```

### `T05a` — Extract Building & Roof Spec from Legacy `street.rs`
* **Tier:** `[Coordinator / Frontier]` *(reads 111 KB `street.rs` so lightweight agents don't have to)*
* **Depends on:** `T04`
* **Read Set:** `crates/zombiebox-render/src/street.rs`, `crates/zombiebox-core/src/action/spatial.rs`
* **Write Set:** `docs/render3d-building-spec.md`
* **Goal:**
  1. Distill the exact wall ownership rules (`AlongX`, `AlongY`, `Corner`), door/window jamb/lintel dimensions, damage stages, stair placement, `BuildingStyle` palettes, and pitched/flat roof + collapse rules into a concise 1–2 page reference doc.
* **Verification:** Coordinator review of `docs/render3d-building-spec.md`.

### `T05b` — Building Walls, Openings, Floors & Stairs
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T05a`
* **Read Set:** `docs/render3d-building-spec.md`, `crates/zombiebox-core/src/action/spatial.rs`, `crates/zombiebox-render3d/src/mesh/mod.rs`
* **Write Set:** `crates/zombiebox-render3d/src/mesh/buildings.rs`, `crates/zombiebox-render3d/src/mesh/mod.rs`, `crates/zombiebox-render3d/tests/geometry.rs`
* **Goal:**
  1. Emit walls from core spatial wall-segment descriptors (split into `WallStub` + `WallUpper` classes for later cutaway), doors (open/closed hinge rotation), windows (intact/broken), interior floors/slabs (respecting collapsed holes), and stairs matching core ramps.
  2. Test door toggle, broken window, breached wall, and collapsed upper slab on Birch Street.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --no-default-features --test geometry --locked
  ```

### `T05c` — Building Roofs, Chunk Clipping & Collapse
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T05b`
* **Read Set:** `docs/render3d-building-spec.md`, `crates/zombiebox-render3d/src/mesh/buildings.rs`
* **Write Set:** `crates/zombiebox-render3d/src/mesh/buildings.rs`, `crates/zombiebox-render3d/tests/geometry.rs`
* **Goal:**
  1. Implement `emit_roof` for pitched (`has_pitched_roof()`) and flat styles, derived from live supported footprints and clipped cleanly to chunk ownership (no duplicate roofs across chunk seams).
  2. Tag emitted roof geometry as `GeometryClass::Roof`; verify removed supporting cell updates/removes roof cleanly.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --no-default-features --test geometry --locked
  cargo run -p zombiebox-render3d --example preview --features gpu --locked -- --scenario birch --seed 42 --camera orbit --size 960x540 --output /tmp/zb-t05-orbit.png
  ```

### `T06` — Props, Caches, Lamps & District Material Palettes
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T05c`
* **Read Set:** `docs/render3d-building-spec.md`, `crates/zombiebox-core/src/action/types.rs` (`PropKind`, `BuildingStyle`)
* **Write Set:** `crates/zombiebox-render3d/src/mesh/props.rs`, `crates/zombiebox-render3d/src/materials.rs`, `crates/zombiebox-render3d/src/mesh/mod.rs`, `crates/zombiebox-render3d/tests/geometry.rs`
* **Goal:**
  1. Build low-poly local meshes (ground-contact origin) for every `PropKind` (including 3D trees with trunk + canopy), streetlights, rubble, and loot caches.
  2. Populate full `MaterialRegistry` covering all `BuildingStyle` palettes.
  3. Ensure cache open/unopened state updates via instance/appearance without triggering terrain mesh rebuilds.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --no-default-features --test geometry --locked
  ```

### `T07a` — Visibility Policy, Cutaway State & Parity Matrix (Pure CPU)
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T06`
* **Read Set:** `crates/zombiebox-render/src/lib.rs` (cutaway/stipple logic only), `crates/zombiebox-render/src/lighting.rs`
* **Write Set:** `crates/zombiebox-render3d/src/scene/visibility.rs`, `crates/zombiebox-render3d/tests/parity.rs`
* **Goal:**
  1. Implement `RenderVisibilityPolicy` (`Gameplay`, `DebugReveal`), `CellAppearance`, `CutawayState`, and `cell_appearance()`.
  2. Replicate legacy indoor/outdoor, F0/F1/F2 cutaway, lower-floor dimming, and `Hidden`/`Explored`/`Visible` shroud semantics.
  3. Write unit tests verifying `Gameplay` never reveals `Hidden` interiors at any camera angle, and idle visibility updates do not mark geometry dirty.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --no-default-features --test parity --locked
  ```

### `T07b` — GPU Appearance Buffer, Dither Cutaway & Lighting Shader
* **Tier:** `[Coordinator / Frontier]` *(WGSL shader + per-chunk appearance GPU buffer)*
* **Depends on:** `T07a`
* **Read Set:** `crates/zombiebox-render3d/src/scene/visibility.rs`, `crates/zombiebox-render3d/src/shaders/world.wgsl`, `crates/zombiebox-render3d/src/gpu/buffers.rs`
* **Write Set:** `crates/zombiebox-render3d/src/shaders/world.wgsl`, `crates/zombiebox-render3d/src/gpu/buffers.rs`, `crates/zombiebox-render3d/src/gpu/pipelines.rs`, `crates/zombiebox-render3d/src/gpu/draw.rs`
* **Goal:**
  1. Upload per-chunk `CellAppearance` buffers each frame (without rebuilding static chunk meshes).
  2. Implement screen-door dither/alpha-test cutaway in `world.wgsl` for `Roof` and `WallUpper` classes (writes depth only for retained fragments).
  3. Implement fixed world directional light + `dusk_level` ambient + per-cell light + bounded dynamic flash list + shroud tint + distance fog near active-window boundary.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --features gpu --locked
  cargo run -p zombiebox-render3d --example preview --features gpu --locked -- --scenario birch --seed 42 --camera iso --size 960x540 --output /tmp/zb-t07-iso.png
  ```

### `T08` — 3D Articulated Actors & Instanced Rendering
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T07b`
* **Read Set:** `crates/zombiebox-core/src/action/types.rs` (`PlayerRenderState`, `MonsterView`), `crates/zombiebox-render3d/src/gpu/buffers.rs`
* **Write Set:** `crates/zombiebox-render3d/src/mesh/actors.rs`, `crates/zombiebox-render3d/src/animation.rs`, `crates/zombiebox-render3d/src/gpu/draw.rs`, `crates/zombiebox-render3d/tests/geometry.rs`
* **Goal:**
  1. Build rigid-part humanoid meshes (`ActorRig`) with distinct proportions/palettes for Survivor, Shambler, Brisk, Bloater, and Shooter, plus hand-attached weapon primitives.
  2. Implement `ActorAnimator` driven by simulation displacement (`observe_tick`), gait swing, hit reaction, telegraph, and corpse drop. Walking animation must not advance on camera-only rotation.
  3. Render actors via GPU instancing grouped by part mesh; hide local player body only in `FirstPerson`.
  4. Test 100-actor stress scene has bounded draw calls.
* **Verification:**
  ```sh
  cargo test -p zombiebox-render3d --features gpu --locked
  ```

### `T09` — Effects, `Presentation` Facade, Camera Obstruction & Desktop Viewer
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T08`
* **Read Set:** `crates/zombiebox-render/src/vfx.rs` (timings/caps only), `crates/zombiebox-render3d/src/lib.rs`, `crates/zombiebox-render3d/src/camera.rs`
* **Write Set:** `crates/zombiebox-render3d/src/effects.rs`, `crates/zombiebox-render3d/src/shaders/effects.wgsl`, `crates/zombiebox-render3d/src/lib.rs`, `crates/zombiebox-render3d/src/camera.rs`, `crates/zombiebox-render3d/examples/viewer.rs`, `crates/zombiebox-render3d/tests/parity.rs`
* **Goal:**
  1. Implement `EffectsState` (`observe_tick` once per sim step, deduplicated by tick, bounded pools for sparks, smoke, debris, blood, tracers, flashes, corpses) and 3D projectiles using authoritative `position.z`.
  2. Wire `resolve_camera_eye` using `SpatialWorld::sphere_cast` for Chase/Orbit obstruction.
  3. Complete `Presentation` facade (`observe_tick`, `prepare`, `notify_teleport`, `set_camera`, `screen_stick_to_world`, `mark_presented`).
  4. Build interactive desktop `examples/viewer.rs` (keys `1–5` switch cameras live on the same run; arrows adjust yaw/pitch/zoom; live combat/movement; HUD stats).
* **Verification (Desktop E2E Milestone!):**
  ```sh
  cargo test -p zombiebox-render3d --features gpu --locked
  cargo run -p zombiebox-render3d --example viewer --features viewer --locked -- --seed 42
  ```
  *Coordinator manual check:* Walk, fight, open doors, cross hills/bridges, and switch all 5 camera modes live in `viewer` without run resets or visual defects.

---

## Phase 3: Host Integrations (Web & Android)

### `T12` — Browser GPU Action Host (`zombiebox-web`)
*(Pulled ahead of Android QA so desktop/browser e2e is verified before touching mobile)*
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T09`
* **Read Set:** `crates/zombiebox-web/Cargo.toml`, `crates/zombiebox-web/src/main.rs`
* **Write Set:** `crates/zombiebox-web/Cargo.toml`, `crates/zombiebox-web/src/main.rs`, minimal camera selector in `apps/web/action.js` if needed
* **Goal:**
  1. Replace per-session CPU `Renderer` in `zombiebox-web` with `Presentation` + shared headless `GpuRenderer` device/queue and per-run targets.
  2. Preserve ZBF2 RGBA wire format, fixed ticks, input flags, audio masks, atlas, and `/legacy`.
  3. Ensure GPU readback does not hold a global lock blocking unrelated sessions; return explicit error if no GPU adapter is present.
* **Verification:**
  ```sh
  cargo test -p zombiebox-web --locked
  ```
  *Coordinator manual check:* Play seed 42 in browser via ZBF2 and switch cameras.

### `T10a` — Additive Rust FFI & C Header (`zb_action_v3_*`)
* **Tier:** `[Lightweight OK]`
* **Depends on:** `T09`
* **Read Set:** `crates/zombiebox-ffi/Cargo.toml`, `crates/zombiebox-ffi/src/action.rs`, `crates/zombiebox-ffi/include/zombiebox.h`, `crates/zombiebox-ffi/tests/host.c`
* **Write Set:** `crates/zombiebox-ffi/Cargo.toml`, `crates/zombiebox-ffi/src/gpu_action.rs`, `crates/zombiebox-ffi/src/action.rs`, `crates/zombiebox-ffi/src/lib.rs`, `crates/zombiebox-ffi/include/zombiebox.h`, `crates/zombiebox-ffi/tests/host.c`
* **Goal:**
  1. Add optional `gpu3d` feature to `zombiebox-ffi`.
  2. Add `ZbGpuStatus`, `ZbCameraMode`, `ZbCameraConfigV3`, and `zb_action_v3_*` exports (`attach_android_surface`, `resize`, `detach_surface`, `set_camera`, `frame`) while preserving `v1`/`v2` during migration.
  3. Refactor `advance_v2` to take **world** vectors; `frame_v3` resolves screen stick via 3D `Presentation`, dispatches `observe_tick` once per step, and returns audio mask even on `SKIPPED` frames.
* **Verification:**
  ```sh
  cargo check -p zombiebox-ffi --features gpu3d --locked
  ./scripts/check-native.sh
  ```

### `T10b` — Android JNI, `ANativeWindow` Lifetime & Non-Blocking GPU Init
* **Tier:** `[Coordinator / Frontier]` *(unsafe NDK window refs, `raw-window-handle`, async init thread)*
* **Depends on:** `T10a`
* **Read Set:** `apps/mobile/android/zombiebox_jni.c`, `apps/mobile/android/build-rust.sh`, `apps/mobile/android/app/src/main/java/game/zombiebox/mobile/NativeGame.java`
* **Write Set:** `crates/zombiebox-ffi/src/gpu_action.rs`, `apps/mobile/android/zombiebox_jni.c`, `apps/mobile/android/build-rust.sh`, `apps/mobile/android/app/src/main/java/game/zombiebox/mobile/NativeGame.java`
* **Goal:**
  1. Wire JNI `attachSurfaceV3`, `resizeSurfaceV3`, `detachSurfaceV3`, `setCameraV3`, `frameV3`.
  2. Acquire/release `ANativeWindow` ref cleanly; initialize `wgpu` adapter/device on a worker thread with generation IDs so UI callbacks never block and stale surfaces are discarded.
  3. Update `build-rust.sh` to build `arm64-v8a` and `x86_64` with `--features gpu3d`.
* **Verification:**
  ```sh
  ./apps/mobile/android/build-rust.sh
  ```

### `T10c` — `ZombieboxSurfaceView.kt` & React Native Camera Selector
* **Tier:** `[Coordinator / Frontier]` *(43 KB Kotlin View + RN native component wiring)*
* **Depends on:** `T10b`
* **Read Set:** `apps/mobile/android/app/src/main/java/game/zombiebox/mobile/ZombieboxSurfaceView.kt`, `apps/mobile/specs/ZombieboxSurfaceNativeComponent.ts`, `apps/mobile/App.tsx`
* **Write Set:** `apps/mobile/android/app/src/main/java/game/zombiebox/mobile/ZombieboxSurfaceView.kt`, `apps/mobile/specs/ZombieboxSurfaceNativeComponent.ts`, `apps/mobile/App.tsx`
* **Goal:**
  1. Refactor `ZombieboxSurfaceView` into a `FrameLayout` with a bottom `SurfaceView` (GPU) and top transparent controls/HUD overlay. Keep temporary CPU toggle until `T13`.
  2. Wire `SurfaceHolder.Callback` without destroying `Runs` game state on surface detach/rotation/background/atlas teleport.
  3. Add user-facing camera mode selector (all 5 modes) and explicit error/retry UI if GPU is unavailable.
* **Verification:** Gradle Android debug build succeeds cleanly for both ABIs.

---

## Phase 4: Physical Android QA & Single-Renderer Cutover

### `T11` — Physical Android Handset QA & Performance Tuning
* **Tier:** `[Coordinator / Frontier]`
* **Depends on:** `T10c`
* **Read Set:** `docs/render3d-progress.md`
* **Write Set:** Measured bottleneck fixes only, `docs/render3d-progress.md`
* **Goal:**
  1. Run 10-minute physical handset test on seed 42 (residential, downtown, hospital, woods, bridge, F0–F2 interiors, 100 monsters, chunk crossings, teleports, background/resume, all 5 cameras).
  2. Record adapter/driver, resolution, p50/p95 frame intervals (target p95 $\le 16.7$ ms), static upload count (0 on idle), and memory stability.
  3. Tune render scale / culling / batching if needed (never shrink simulation or difficulty). Flip GPU to default once accepted.
* **Verification:** Documented physical handset telemetry in `docs/render3d-progress.md`.

### `T13` — Mandatory Single-Renderer Cutover
* **Tier:** `[Lightweight OK]` *(mechanical cleanup after T11 & T12 gates pass)*
* **Depends on:** `T11`, `T12`
* **Read Set:** Workspace `Cargo.toml`, `crates/zombiebox-ffi/*`, `apps/mobile/*`, `docs/*`
* **Write Set:** Delete `crates/zombiebox-render/`, remove legacy bitmap action ABI (`zb_action_v1_*` / `v2` bitmap frame), remove `AndroidBitmap_lockPixels` action path, remove temporary CPU renderer toggle and `S03a` shims, update `docs/architecture.md`, `docs/product-decisions.md`, `docs/goals.md`, `README.md`.
* **Goal:** Leave `zombiebox-render3d` as the sole production action renderer on Android and Web. Update product docs to mark 3D prerequisite complete (do not check off future vehicle goals).
* **Verification:**
  ```sh
  rg "zombiebox-render[^3]" crates/ apps/
  rg "AndroidBitmap_lockPixels" apps/mobile/
  cargo fmt --all -- --check
  cargo test --workspace --locked
  cargo clippy -p zombiebox-render3d --all-targets --features gpu --locked -- -D warnings
  ./scripts/check-native.sh
  ```
  *Final gate:* Repeat quick physical handset + browser smoke test. Vehicles may begin after this gate passes.