# Zombiebox 3D Migration — Progress Ledger

* **Baseline Commit:** `daeff4a`  
* **Toolchain / Environment:** `rustc 1.100.0`, `wgpu 30.0.1` (`0ef1bc386 2026-11-09`, MSRV `1.87`, `glam 0.27.0`, `bytemuck 1.25.2`, `winit 0.30.13`, `png 0.18.1`)
* **Current Phase:** `Phase 1 (S01)`

---

## Task Status

| Milestone | Title | Status | Commit | Notes / Validation |
|---|---|---|---|---|
| `T00` | Baseline Capture & Desktop GPU Setup | DONE | — | 11 reference PNGs archived in `docs/baselines/iso-2p5d/`; `zombiebox-render3d` CPU + headless GPU cube smoke test verified |
| `S01` | Spatial Primitives, Terrain Grid & World Relief | TODO | — | Continuous elevation, flat building pads, road grading |
| `S02` | Spatial Supports, Collision & Kinematics | TODO | — | Continuous support resolution, collision queries, kinematic sweep |
| `S03` | Core Simulation 3D Migration | TODO | — | Player, entity poses, monster AI navigation, 3D weapons & events |
| `T01` | Coordinates, Camera Rigs & Input Mapping | TODO | — | Projections, rigs, iso calibration, analog stick input mapping |
| `T02` | Scene Management & Chunk Invalidation Cache | TODO | — | Scene snapshot synchronization & dirty chunk caching |
| `T03` | Mesh Primitives & GPU Render Pipeline | TODO | — | `wgpu` pipeline, mesh builders, low-res target & retro upscale |
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
