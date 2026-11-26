# Zombiebox 3D — Architecture & Contracts Reference

**Purpose:** Compact, stable technical contracts for coding agents. Keep this file small so lightweight models can load it alongside a single task card without context bloat.  
**Scope:** Authoritative 3D spatial simulation in `zombiebox-core` + single `wgpu` 3D renderer in `zombiebox-render3d`. Default play remains 2:1 isometric; Overhead, Orbit, Chase, and FirstPerson run on the same 3D scene and live run.  
**Non-goals (deferred until after T13):** Vehicles, vehicle art/physics, buoyancy/swimming, flight, dedicated FPS combat/viewmodels, PBR, real-time shadow maps, skeletal animation, WebGPU client rewrite, iOS port.

---

## 1. Toolchain & Crate Boundaries

* **Rust MSRV:** `1.87.0` (verify with `cargo +1.87.0 check --locked`; do not silently bump).
* **GPU Stack:** `wgpu = "26.0.1"` (compatibility-first pin; verify in `T00a` and lock in `Cargo.lock`), `glam`, `bytemuck`, `pollster` (desktop tests/preview only; never block Android UI thread).
* **Dependency direction:** `zombiebox-core` $\leftarrow$ `zombiebox-render3d` $\leftarrow$ `zombiebox-ffi` / `zombiebox-web`.
* **Strict isolation:**
  * `zombiebox-core` has **zero** GPU, `glam`, `wgpu`, or platform dependencies.
  * `zombiebox-render3d` has **zero** dependencies on `zombiebox-ffi`, `zombiebox-render` (legacy CPU), Android, or web hosts.
  * `zombiebox-render3d` must build and pass CPU unit tests with `--no-default-features`. GPU code lives behind `--features gpu`; desktop interactive windowing (`winit`) lives behind `--features viewer`.
* **Single renderer rule:** `zombiebox-render` stays untouched as a temporary reference until `T13` deletes it. Do not build a permanent dual-renderer abstraction.

### Target Crate Layout (`crates/zombiebox-render3d/`)

```text
src/
  lib.rs              # Presentation, PreparedFrame, public exports
  coords.rs           # Core <-> Renderer units, tile bounds
  camera.rs           # CameraMode, CameraRig, CameraFrame, input mapping
  scene/
    mod.rs            # SceneState, sync(), bounds
    snapshot.rs       # Owned FrameSnapshot, TileSnapshot, ChunkSnapshot
    chunks.rs         # ChunkKey, CellKey, TileLookup, NeighborTiles, diffs
    visibility.rs     # RenderVisibilityPolicy, CutawayState, CellAppearance
  mesh/
    mod.rs            # Vertex, MeshData, MeshBuilder, ChunkMesh
    primitives.rs     # Quad, cuboid, cylinder, wedge, transforms
    terrain.rs        # Surfaces, roads, bridges, water, mountains
    buildings.rs      # Walls, openings, floors, stairs, roofs, damage
    props.rs          # PropKind meshes, caches, lamps, rubble
    actors.rs         # Rigid-part humanoid meshes & rigs
  animation.rs        # ActorAnimator, gait tracks, event reactions
  effects.rs          # EffectsState, projectiles, particles, decals
  materials.rs        # Palette/material registry, sRGB -> linear
  gpu/                # Behind #[cfg(feature = "gpu")]
    mod.rs            # GpuRenderer, RenderConfig, RenderStats, RenderError
    context.rs        # Instance, Adapter, Device, Queue
    buffers.rs        # Static chunk buffers, dynamic instance buffers
    pipelines.rs      # Opaque, cutout/dither, effects, upscale pipelines
    targets.rs        # Low-res color + Depth32Float, surface/offscreen
    draw.rs           # Frame encoding, frustum culling
    readback.rs       # 256-byte aligned staging readback (preview/web only)
  shaders/
    world.wgsl
    effects.wgsl
    upscale.wgsl
```

---

## 2. Coordinate System & Camera Math

* **Core coordinates:** `(x, y, z)` where `x, y` are horizontal and `z` is vertical up (world units). Tile centers are integers `(x, y)`; tile edges are at $\pm 0.5$.
* **Renderer coordinates:** `(X, Y, Z) = (x, z, y)` with `+Y` up.
  * **Handedness warning:** Swapping `y` and `z` flips handedness. Always emit outward-facing triangles with **CCW winding** in renderer space (`(p1 - p0).cross(p2 - p0).dot(normal) > 0`).
* **Floor height:** `pub const FLOOR_H: f32 = 1.388;` defined in `zombiebox_core::spatial` and re-exported by `zombiebox-render3d`. Building floor elevation is `foundation_z + floor * FLOOR_H`.
* **Depth range:** Right-handed matrices with **$[0, 1]$ depth** (`glam::Mat4::look_to_rh`, `orthographic_rh`, `perspective_rh`).
* **Default Isometric calibration:**
  * `yaw` $= \pi / 4$, `pitch` $= \pi / 6$ (depression from horizontal).
  * `right = (cos(yaw), 0, -sin(yaw))`, horizontal forward $= (-\sin(\text{yaw}), 0, -\cos(\text{yaw}))$, `up = right.cross(forward)` (avoids `look_at` singularity at overhead $\text{pitch} = \pi / 2$).
  * Scale: `28.284 px / unit` (`vertical_span = internal_height / 28.284`).
  * **Calibration assertion:** `+X` step $\approx (+20, +10)$ px, `+Z` step $\approx (-20, +10)$ px, `+FLOOR_H` up $\approx (0, -34)$ px.
* **Stick input mapping:**
  * Default iso preserves legacy screen-stick mapping `(sx/20 + sy/10)/2, (sy/10 - sx/20)/2`, normalized and scaled by $\min(\text{hypot}(sx, sy), 1.0)$.
  * Overhead/Chase/FirstPerson use yaw-relative planar right/forward vectors.
  * Host always converts stick input against the **last presented** `CameraFrame` before ticking core.
* **Damping:** Exponential half-life $a = 1 - 2^{-\Delta t / h}$; wrap yaw via shortest angle; snap on teleport or run reset.

---

## 3. Core Spatial Contracts (`zombiebox-core/src/spatial/`)

```rust
// crates/zombiebox-core/src/spatial/mod.rs
pub const FLOOR_H: f32 = 1.388;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 { pub x: f32, pub y: f32, pub z: f32 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb3 { pub min: Vec3, pub max: Vec3 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyShape { pub radius: f32, pub height: f32 }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SupportId { Terrain, Floor(Tile3D), Stair(Tile3D), Bridge(Position) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionState { Grounded(SupportId), Airborne }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyState {
    pub position: Vec3, // Foot/contact position, NOT capsule center
    pub velocity: Vec3, // World units/s
    pub heading: f32,   // Radians: 0 = +world y, +heading toward +world x
    pub pitch: f32,
    pub roll: f32,
    pub motion: MotionState,
}
```

### Terrain Grid (`spatial/terrain.rs` & `world/relief.rs`)

* Single immutable global **$513 \times 513$** vertex grid for the $512 \times 512$ county, indexed at `(i - 0.5, j - 0.5)` row-major, quantized as `i16` in $1/256$ world-unit steps.
* Birch Street fixture uses flat `0.0` elevation.
* **Exact triangle split (shared by physics and renderer):**
  * Cell: `cx = floor(x + 0.5)`, `cy = floor(y + 0.5)`, local `u = x - (cx - 0.5)`, `v = y - (cy - 0.5)`.
  * Corner order: `[h00, h10, h11, h01]` = SW, SE, NE, NW.
  * Fixed diagonal `00 -> 11`:
    * If $u \ge v$: $z = (1 - u) h_{00} + (u - v) h_{10} + v h_{11}$.
    * Else: $z = (1 - v) h_{00} + u h_{11} + (v - u) h_{01}$.
  * Same split applies to bridge decks and river water surfaces.
* **Generation invariants:** Building pads are flat and order-independent; required roads meet max grade `0.20`; river water is non-increasing downstream; water bed $\ge 0.25$ below water; bridge deck $\ge 0.25$ above water.

### Supports & Collision (`spatial/support.rs` & `spatial/collision.rs`)

```rust
pub struct SupportSurface {
    pub id: SupportId, pub elevation: f32, pub normal: Vec3,
    pub surface: Surface, pub walkable: bool,
}
pub struct ColliderId(pub u64);
pub enum ColliderShape { Box(Aabb3) }
pub struct Collider { pub id: ColliderId, pub shape: ColliderShape, pub tile: Tile3D }
pub struct Ray3 { pub origin: Vec3, pub direction: Vec3 }
pub struct Hit3 { pub distance: f32, pub point: Vec3, pub normal: Vec3, pub collider: Option<ColliderId> }
pub struct MoveResult { pub position: Vec3, pub velocity: Vec3, pub motion: MotionState, pub hit: Option<Hit3> }

pub trait SpatialWorld {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample>;
    fn supports_at(&self, xy: Vec2, out: &mut Vec<SupportSurface>);
    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>);
    fn raycast(&self, ray: Ray3, max_distance: f32) -> Option<Hit3>;
    fn sphere_cast(&self, from: Vec3, to: Vec3, radius: f32) -> Option<Hit3>;
}
```

* **Walkable support rule:** Select the reachable support nearest current foot `z` within step-up (`0.2`) / step-down (`0.3`) bounds. Never pick the highest surface blindly (prevents snapping onto a bridge deck from underneath).
* **Cutaways never affect physics.** Collision derives from live `TileRenderInfo` + core wall segment descriptors.

---

## 4. Renderer Contracts (`zombiebox-render3d`)

### Vertex & Appearance Layout

```rust
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryClass {
    Ground = 0, Interior = 1, ExteriorWall = 2, Roof = 3,
    Slab = 4, Prop = 5, Actor = 6, WallStub = 7, WallUpper = 8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub material_id: u32,
    pub cell_id: u32,        // chunk-local index; sentinel u32::MAX for per-instance
    pub geometry_class: u32, // GeometryClass as u32
}
```

### Presentation vs GPU Facade

* `Presentation` (CPU, per-run): owns `SceneState`, `CameraRig`, `ActorAnimator`, `EffectsState`, `CutawayState`, and last presented `CameraFrame`.
  * `observe_tick(game, sim_time_ms)` **must** be called once immediately after every `game.step()` (before the next step clears `game.events()`).
  * `prepare(game, sim_time_ms, viewport) -> PreparedFrame<'_>` extracts dynamic instances and dirty chunk deltas.
* `GpuRenderer` (GPU, device-lifetime): owns `wgpu` device/queue, pipelines, low-res render targets, static chunk vertex/index buffers, and shared actor/prop meshes.
  * Tracks per-chunk geometry revision/hash (never assumes a one-shot delta was consumed if a frame was skipped).
  * Idle frames with no geometry mutation must perform **zero** static chunk rebuilds or GPU uploads.
  * Low-res color + `Depth32Float` pass upscaled via nearest-neighbor full-screen triangle.

---

## 5. Key Invariants (Never Break)

1. **Camera independence:** Switching between Isometric, Overhead, Orbit, Chase, and FirstPerson must never mutate `ActionGame`, regenerate chunk geometry, alter collision/AI/visibility, or change simulation outcomes for identical world-space inputs.
2. **Event integrity:** Audio event masks (`events_to_sfx_mask`) remain authoritative; zero-tick frames never duplicate events; multi-tick frames never drop events.
3. **Chunk seams:** Chunk coordinates use `div_euclid(16)` / `rem_euclid(16)`. Adjacent chunk edges share identical terrain vertices and propagate 1-tile halo invalidation on mutation.
4. **Visibility parity:** Gameplay cameras must never expose `Hidden` interiors via camera rotation or mode switching. `DebugReveal` is strictly opt-in for dev/viewer use.