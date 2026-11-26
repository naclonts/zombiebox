# Zombiebox 3D — Architecture & Guidelines Reference

## Overview & Scope

This document outlines the architectural conventions and technical guidelines for the Zombiebox 3D migration. 

* **Simulation:** Authoritative 3D spatial simulation in `zombiebox-core` (terrain elevation, continuous kinematics, multi-floor movement, 3D line-of-sight and projectiles).
* **Rendering:** A modern `wgpu` 3D renderer in `zombiebox-render3d`. Default gameplay remains 2:1 isometric, while supporting multiple perspectives (Overhead, Orbit, Chase, FirstPerson) on the same unified 3D simulation.
* **Deferred Scope:** Advanced vehicle mechanics, dedicated FPS viewmodels, real-time shadow cascades, and skeletal meshes can be layered in after the core 3D pipeline is established.

---

## 1. Crate Boundaries & Guidelines

### Dependency Direction
`zombiebox-core` $\leftarrow$ `zombiebox-render3d` $\leftarrow$ `zombiebox-ffi` / `zombiebox-web`

* **`zombiebox-core`**: Focuses purely on simulation state, geography, and spatial kinematics. It should remain free of GPU dependencies, windowing crates, or rendering APIs.
* **`zombiebox-render3d`**: Handles scene synchronization, mesh generation, camera control, and GPU rendering. It should remain decoupled from platform hosts (Android/Web) and legacy CPU rendering.
* **Feature Flags**: It is helpful to gate GPU and windowing code behind features (e.g. `gpu` for `wgpu`, `viewer` for desktop `winit`), allowing core data types and math to be tested quickly on CPU without requiring a GPU adapter.
* **Migration Strategy**: The existing CPU renderer (`zombiebox-render`) serves as a reference during development. The end goal is to replace it with `zombiebox-render3d` across desktop, web, and mobile once validated.

### Suggested Module Organization (`crates/zombiebox-render3d/`)

This suggested structure provides a clean separation of concerns:

```text
src/
  lib.rs              # Public exports and presentation facade
  coords.rs           # World/tile conversion helpers and bounds
  camera.rs           # Camera modes, rigs, projection math, and input mapping
  scene/              # Scene state snapshots, chunk caching, visibility/cutaways
  mesh/               # Geometry builders: terrain, buildings, props, actors
  animation.rs        # Actor animation, gait tracks, and event reactions
  effects.rs          # Particle systems, decals, and dynamic effects
  materials.rs        # Palette registry and material properties
  gpu/                # GPU device management, pipelines, buffers, and draw encoding
  shaders/            # WGSL shader sources (world, effects, upscale)
```

---

## 2. Coordinate System & Camera Guidelines

* **Simulation Coordinates (`zombiebox-core`):** `(x, y, z)` where `x` and `y` define the horizontal plane and `+z` is vertical elevation. Tile centers sit at integer coordinates `(x, y)`.
* **Renderer Coordinates (`zombiebox-render3d`):** `(X, Y, Z) = (x, z, y)` with `+Y` representing up.
  * *Winding convention:* To maintain outward-facing triangles across the coordinate flip, use counter-clockwise (CCW) winding in renderer space.
* **Floor Elevation:** Floor height convention is `FLOOR_H ≈ 1.388` world units (re-exported from `zombiebox-core`). Building floors elevate as `foundation_z + floor * FLOOR_H`.
* **Depth & Matrices:** Prefer standard right-handed matrices with $[0, 1]$ depth range (e.g. `glam`'s `look_to_rh`, `perspective_rh`, `orthographic_rh`).
* **Isometric Baseline Calibration:**
  * Standard 2:1 isometric corresponds to `yaw = π / 4` and `pitch = π / 6` (depression from horizontal).
  * Approximate pixel scale: `28.284 px / unit` (`vertical_span = internal_height / 28.284`).
  * Expected screen-space stepping: `+X` maps to approx `(+20, +10)` px, `+Z` to `(-20, +10)` px, and `+FLOOR_H` vertical climb to `(0, -34)` px.
* **Input Mapping:**
  * For isometric view, stick inputs align with legacy screen axes: `(sx/20 + sy/10)/2, (sy/10 - sx/20)/2`.
  * For directional modes (Chase, FirstPerson, Overhead), stick inputs map relative to camera yaw.
  * Host platforms should convert analog stick inputs using the most recent camera frame before advancing simulation ticks.

---

## 3. Core Spatial Simulation

### Core Spatial Types (`zombiebox-core::spatial`)

A lightweight spatial representation helps unify continuous collision and terrain sampling:

```rust
pub const FLOOR_H: f32 = 1.388;

pub struct Vec3 { pub x: f32, pub y: f32, pub z: f32 }
pub struct Aabb3 { pub min: Vec3, pub max: Vec3 }
pub struct BodyShape { pub radius: f32, pub height: f32 }

pub enum SupportId { Terrain, Floor(Tile3D), Stair(Tile3D), Bridge(Position) }
pub enum MotionState { Grounded(SupportId), Airborne }

pub struct BodyState {
    pub position: Vec3, // Ground contact / foot position
    pub velocity: Vec3,
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub motion: MotionState,
}
```

### Terrain Sampling & Hydrology
* The county terrain is represented on a continuous height grid (e.g. $513 \times 513$ grid for a $512 \times 512$ county).
* A consistent diagonal split (e.g. `00 -> 11`) ensures height interpolation and normal derivation match between simulation collision and rendering.
* Flat building pads (`foundation_z`), reasonable road grades ($\le 0.20$), and riverbeds positioned below water surface lines keep geography plausible and navigable.

### Spatial Query Interface (`SpatialWorld`)
Exposing spatial queries through a trait or service lets kinematics, raycasts, and AI share common ground:

```rust
pub trait SpatialWorld {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample>;
    fn supports_at(&self, xy: Vec2, out: &mut Vec<SupportSurface>);
    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>);
    fn raycast(&self, ray: Ray3, max_distance: f32) -> Option<Hit3>;
    fn sphere_cast(&self, from: Vec3, to: Vec3, radius: f32) -> Option<Hit3>;
}
```

* **Support Selection:** Grounded entities select the nearest reachable walkable surface within reasonable step tolerances (~0.2 step-up / ~0.3 step-down) rather than blindly picking the highest elevation (preventing unwanted snaps onto bridge decks from below).
* **Kinematics & Destruction:** Dynamic wall breaches and opening doors should immediately update collision geometry. Cutaways and camera shroud effects are visual-only and do not alter physical collision.

---

## 4. Presentation & Renderer Guidelines

### CPU Presentation vs. GPU Renderer
Separating presentation state from device-level rendering keeps platform integration clean:

* **Presentation Layer (CPU):**
  * Maintains camera rigs, actor animation state, cutaways, and particle effect pools.
  * Observes simulation ticks (`observe_tick`) to capture transient events (gunshots, explosions, footfalls) before they are cleared.
  * Prepares render snapshots (`prepare`) containing dirty chunk deltas and dynamic instance transforms.
* **GPU Renderer (Device):**
  * Manages `wgpu` pipelines, static vertex/index buffers, dynamic instance buffers, and render targets.
  * Tracks chunk revisions or fingerprints to avoid rebuilding static geometry when only the camera moves.
  * Renders to an internal low-resolution target with depth testing, then upscales to the presentation surface using nearest-neighbor sampling for a crisp retro aesthetic.

---

## 5. Core Design Principles

1. **Camera Independence:** Camera mode switching (Isometric, Overhead, Orbit, Chase, FirstPerson) is a presentation choice. It should never alter simulation outcomes, AI behavior, or world state for equivalent inputs.
2. **Simulation Event Integrity:** Audio and particle effects rely on event streams emitted during simulation ticks. Zero-tick frames should not duplicate effects, and multi-tick catchup frames should process all events.
3. **Seamless Chunk Boundaries:** Chunk edges should align without visible cracks or normal seams. Invalidation on world modification should propagate to neighboring chunk halos where appropriate.
4. **Gameplay Visibility Rules:** Fog-of-war and room discovery rules should be respected across all gameplay perspectives. Internal rooms marked as hidden should not be exposed simply by orbiting the camera.
