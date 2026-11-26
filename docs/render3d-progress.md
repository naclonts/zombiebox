# Zombiebox 3D Migration — Progress Ledger

* **Baseline Commit:** `daeff4a`  
* **Toolchain / Environment:** `rustc 1.100.0`, `wgpu 30.0.1` (`0ef1bc386 2026-11-09`, MSRV `1.87`, `glam 0.27.0`, `bytemuck 1.25.2`, `winit 0.30.13`, `png 0.18.1`)
* **Current Phase:** `Phase 2 (T03 Complete)`

---

## Task Status

| Milestone | Title | Status | Commit | Notes / Validation |
|---|---|---|---|---|
| `T00` | Baseline Capture & Desktop GPU Setup | DONE | `b8ddaf0` | 11 reference PNGs archived in `docs/baselines/iso-2p5d/`; `zombiebox-render3d` CPU + headless GPU cube smoke test verified |
| `S01` | Spatial Primitives, Terrain Grid & World Relief | DONE | `91e741c` | `zombiebox_core::spatial` (`Vec2`, `Vec3`, `Aabb3`, `TerrainGrid`, `sample_quad_00_11`), continuous `513×513` county relief, flat `foundation_z` pads, `<= 0.20` road grades, submerged waterbeds, and flat Birch Street queries verified across 8 seeds (`cargo test --workspace`, `gpu`, `clippy`, `fmt` clean) |
| `S02` | Spatial Supports, Collision & Kinematics | DONE | `fbcb1bb` | `SpatialWorld`, `StairGeometry` (`3×3` straight & L-turn winder + `StairsBoth`), anti-snap support selection, Minkowski circle-AABB CCD + adaptive kinematic sweep, watertight `raycast`/`sphere_cast` with window/door apertures, and `O(1)` `ActionSpatialView` / `WorldSpatialView` adapters (`spatial_kinematics` + workspace verification) |
| `S03` | Core Simulation 3D Migration | DONE | `6a2f00f` | `CombatWorld: SpatialWorld`, 3D `ActionEvent` / `PlayerRenderState` / `MonsterView` / `ProjectileView` / `CacheView` / `LightSourceView` / `BlastView` / `DynamicFlashView`, `ActionGame` 3D kinematic movement/stairs/window drops/fall injury, 3D monster eye-to-eye LOS, acoustic attenuation & Shooter 3D targeting, 3D hitscan/sphere-cast projectiles/occluded blasts, downstream compatibility shims, and `tests/action_3d.rs` |
| `T01` | Coordinates, Camera Rigs & Input Mapping | DONE | `1c9eca7` | `coords.rs` (`Sim<->Render` axes, `ChunkCoord`, `RenderAabb`, CCW helpers, `FLOOR_H` re-export) & `camera.rs` (5 camera modes, `(+20,+10)`/`(-20,+10)`/`(0,-34)` iso calibration, `[0,1]` Gribb-Hartmann frustum, rays, stick mapping, 224B GPU uniforms) verified offline (`cargo test` CPU + `gpu`, `viewer` check, `clippy`, `fmt` clean) |
| `T02` | Scene Management & Chunk Invalidation Cache | TODO | — | Scene snapshot synchronization & dirty chunk caching |
| `T03` | Mesh Primitives & GPU Render Pipeline | DONE | — | `wgpu` pipeline, `MeshBuilder`, primitive emitters (`emit_box`, `emit_cylinder`, `emit_cone`, `emit_quad`), `DynamicInstanceBuffer`, offscreen/surface render targets, internal resolution scaling & retro upscale, and `preview3d` diorama export verified (`cargo test -p zombiebox-render3d --features gpu`, `clippy -D warnings`, `cargo fmt`) |
| `T04` | 3D Terrain & Surface Meshing | TODO | — | County terrain, roads, water planes, and bridge decks |
| `T05` | 3D Buildings, Openings & Destruction | TODO | — | Multi-floor buildings, walls, openings, stairs, roofs & collapse |
| `T06` | Props, Caches & District Palettes | TODO | — | Props, streetlights, loot caches, material registries |
| `T07` | Visibility, Cutaways & Shading | TODO | — | Interior cutaways, dither shading, lighting, and shroud rules |
| `T08` | Articulated Actors & Instanced Animation | TODO | — | Character rigs, gait animation, combat telegraphs, GPU instancing |
| `T09` | Effects, Obstruction & Interactive Desktop Viewer | TODO | — | Dynamic effects, camera obstruction, interactive viewer app |
| `T10` | Android Native Integration | TODO | — | FFI, JNI surface management, Native SurfaceView |
| `T11` | Handset QA & Performance Tuning | TODO | — | Physical Android device profiling, frame pacing, optimization |
| `T12` | Web GPU Action Host | TODO | — | Browser streaming session support for 3D engine |
| `T13` | Legacy Cutover & Cleanup | TODO | — | Deprecate legacy CPU renderer, clean up shims, final verification |

---

## Reference Constants & Decisions

* **County Generator Seed Reference:** Seed `42` for baseline visual and spatial review.
* **Camera Defaults:** 2:1 isometric standard (`yaw = π/4`, `pitch = π/6`, vertical scale ~28.28 px/unit).
* **Vertical Floor Height:** `FLOOR_H ≈ 1.388` world units.
* **Target Frame Interval:** 60 FPS (p95 $\le 16.7$ ms) on target mobile hardware.

---

## Session Log

* **`T00` (Baseline Capture & Desktop GPU Setup):** Archived 11 deterministic `640×360` RGB8 PNG baselines (`docs/baselines/iso-2p5d/`, `576K` total including `manifest.md`) from `zombiebox-render` covering Birch Street (4 scenes) and County Seed 42 (spawn, downtown, hospital, police, army base, woods cabin, river bridge). Scaffolded `crates/zombiebox-render3d` on `wgpu 30.0.1` + `winit 0.30.13` (`wayland-csd-adwaita-crossfont` for offline compatibility) and verified headless offscreen 3D cube rendering, row-padding readback, and PNG roundtrip on `NVIDIA GeForce RTX 2070` (`Vulkan`, `DiscreteGpu`). Ready for `S01` and `T01`.
* **`S01` (Spatial Primitives, Terrain Grid & World Relief):** Added zero-dependency `zombiebox_core::spatial` (`Vec2`, `Vec3`, `Aabb3`, `BodyShape`, `BodyState`, `SupportId`, `MotionState`, `TerrainGrid`, `TerrainSample`, `TerrainTriangle`, `sample_quad_00_11`, `FLOOR_H = 1.388`, `WATER_SURFACE_Z = 0.0`, `MAX_ROAD_GRADE = 0.20`, `MIN_BRIDGE_CLEARANCE = 0.90`), continuous `513×513` county relief synthesis (`world/relief.rs`) with 2-pass Dijkstra road grade clamping, quantized flat building pads (`foundation_z` on `Building` and `BuildingInfo`), submerged riverbeds/lakebeds, bridge deck quads, and chunk/world terrain query APIs while keeping `World::birch_street` flat at `0.0` and preserving lazy `Parcels`. Verified 62 `zombiebox-core` tests (49 unit + 4 `county` + 7 `spatial_relief` + 2 `survival`), full workspace + `zombiebox-render3d --features gpu` tests, `clippy -D warnings`, and `rustfmt`. Ready for `S02` and `T01`.
* **`T01` (Coordinates, Camera Rigs & Input Mapping):** Implemented `crates/zombiebox-render3d/src/coords.rs` (bridging `zombiebox_core::{FLOOR_H, Vec3, Aabb3}` into right-handed `(X, Y, Z) = (x, z, y)` render space, `ChunkCoord`, `RenderAabb`, heading quats, and CCW winding helpers) and `src/camera.rs` (all 5 `CameraMode` presets with singularity-free analytical basis, exact 2:1 isometric pixel calibration `(+20, +10)` / `(-20, +10)` / `(0, -34)`, WebGPU `[0, 1]` Gribb–Hartmann frustum culling, ray unprojection, frame-rate-independent damping, display-to-internal resolution scaling, 224B GPU uniforms, and screen-relative `map_stick_to_world` preserving analog tilt). Verified offline with `zombiebox-render3d` CPU tests, `--features gpu` smoke test, `--features viewer` check/clippy, workspace `clippy -D warnings`, and `cargo fmt --check`. Ready for `T02`.
* **`S02` (Spatial Supports, Collision & Kinematics):** Implemented `crates/zombiebox-core/src/spatial/{query,support,collision,kinematics,action_adapter}.rs` (`SpatialWorld`, `SupportSurface`, `Collider`/`ColliderKind`, `Ray3`/`Hit3`/`QueryFilter`, `SurfaceQuad`, `StairGeometry` `3 × 3` straight & L-turn winder patches including `StairsBoth`, anti-snap `select_grounded_support` / `sweep_airborne_support`, watertight `00 -> 11` ray-triangle, `intersect_ray_capsule`, `sweep_sphere_aabb3`, `sweep_sphere_triangle`, `sweep_circle_rect_2d` CCD + adaptive `step_kinematic_body` with headroom/slab stair clearance, and `O(1)` `ActionSpatialView` / `WorldSpatialView` adapters with `ActionGame::{spatial, toggle_door, damage_tile}`). Added `tests/spatial_kinematics.rs` covering anti-snap bridges/overhangs, ramps/curbs/cliffs/ceilings, high-speed thin-wall CCD, window/door apertures, Birch Street stairs/drops/collapse, and County Seed 42 bridge underpasses and 3-story Apartments stairs. Ready for `S03` and `T02`.
* **`S03` (Core Simulation 3D Migration):** Upgraded `CombatWorld: SpatialWorld` (`ResidentialBlock`, `CountyStage`, `Stage`, `ai::TestWorld`, `weapons::MockWorld`) and factored `SpatialStageSource` into `spatial/action_adapter.rs` alongside `resolve_tile_support`, `resolve_actor_support`, and `semantic_floor_for_body`. Upgraded `ActionEvent`, `NoiseEvent`, `PlayerRenderState` (`body`, `aim_dir`, `muzzle_pos`), `MonsterView` (`body`, `eye_pos`, `telegraph_origin`, `telegraph_dir3`, `telegraph_target3`), `ProjectileView` (`position`, `velocity`, `radius`), `CacheView` (`world_pos`), `LightSourceView` (`position`), plus `BlastView` and `DynamicFlashView`. Migrated `ActionGame` locomotion to 3D `move_kinematic_with_horizontal_vel` with smooth stair traversal/switchback fallback, `WindowClimbed`/`Footstep`/`ActorLanded` events, 1-story (`0` HP) vs 2-story (`1` HP) vs cliff (`>= 3` HP) fall injury, and `Collapse` airborne drops. Upgraded `ai.rs` (`Monster::body` + `ensure_body_synced`, eye-to-eye `LINE_OF_SIGHT` & stair glimpse, 3D noise barrier attenuation, slope-aware A*, kinematic movement, melee vertical/LOS gate, and 3D Shooter bolts) and `weapons.rs` (`compute_player_aim_3d`, 3D cylinder hitscan & glass piercing, `sphere_cast` projectiles, and `LINE_OF_SIGHT`-occluded 3D rocket/bloater blasts). Updated downstream shims in `zombiebox-render`, `zombiebox-ffi`, and `zombiebox-web`, and added `crates/zombiebox-core/tests/action_3d.rs`. Ready for `T02` / `T03`.
