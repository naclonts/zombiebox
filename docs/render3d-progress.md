# Zombiebox 3D Migration — Progress Ledger

**Baseline Commit:** *(fill in from `git rev-parse --short HEAD`)*  
**Rust Toolchain:** `1.87.0`  
**Pinned `wgpu` Version:** *(verify & lock in `T00a`)*  
**Current Phase:** Phase 0 (`T00a`)

---

## Task Status

| Task | Tier | Status | Commit | Notes / Blockers |
|---|---|---|---|---|
| `T00a` | Frontier | TODO | — | Baseline screenshots, `wgpu` pin, desktop triangle PNG |
| `S01a` | Lightweight | TODO | — | Spatial primitives + 513×513 grid + triangle sampler |
| `S01b` | Frontier | TODO | — | Relief, flat building pads, road grading (64 seeds) |
| `S01c` | Lightweight | TODO | — | Lakes, rivers, bridge decks, `World` API |
| `S02a` | Lightweight | TODO | — | Support/collision traits + synthetic TDD fixtures |
| `S02b` | Lightweight | TODO | — | Support resolver, raycast, sphere-cast |
| `S02c` | Frontier | TODO | — | Kinematic solver + live `ActionGame` spatial adapter |
| `S03a` | Lightweight | TODO | — | `action/types.rs` 3D `Vec3` + compat shims |
| `S03b` | Lightweight | TODO | — | Player/block/county 3D movement |
| `S03c` | Lightweight | TODO | — | `action/ai.rs` 3D navigation & LOS |
| `S03d` | Lightweight | TODO | — | `action/weapons.rs` 3D rays, projectiles & events |
| `T01` | Lightweight | TODO | — | `coords.rs`, `camera.rs`, iso calibration tests |
| `T02` | Lightweight | TODO | — | `scene/` snapshot & chunk invalidation cache |
| `T03a` | Lightweight | TODO | — | `mesh/` primitives, builder, winding tests |
| `T03b` | Frontier | TODO | — | `wgpu` pipelines, upscale, headless `preview` |
| `T04` | Lightweight | TODO | — | 3D terrain meshing (all `Surface` variants) |
| `T05a` | Frontier | TODO | — | Extract `docs/render3d-building-spec.md` from `street.rs` |
| `T05b` | Lightweight | TODO | — | Building walls, openings, floors, stairs |
| `T05c` | Lightweight | TODO | — | Building roofs, chunk clipping, collapse |
| `T06` | Lightweight | TODO | — | Props, caches, lamps, district palettes |
| `T07a` | Lightweight | TODO | — | CPU visibility policy, cutaway state, parity tests |
| `T07b` | Frontier | TODO | — | GPU appearance buffer, dither cutaway, lighting |
| `T08` | Lightweight | TODO | — | 3D rigid-part actors, gait animation, instancing |
| `T09` | Lightweight | TODO | — | Effects, `Presentation`, camera obstruction, `viewer` |
| `T12` | Lightweight | TODO | — | Browser GPU action host (`zombiebox-web` ZBF2) |
| `T10a` | Lightweight | TODO | — | Additive Rust FFI `zb_action_v3_*` + C header |
| `T10b` | Frontier | TODO | — | JNI, `ANativeWindow`, non-blocking GPU init |
| `T10c` | Frontier | TODO | — | `ZombieboxSurfaceView.kt` FrameLayout + RN selector |
| `T11` | Frontier | TODO | — | Physical Android 10-min QA & perf tuning |
| `T13` | Lightweight | TODO | — | Delete `zombiebox-render` & legacy bitmap ABI |

---

## Frozen Decisions & Constants

* **World Generator Version / Hash:** *(record in `S01c`)*
* **Desktop Reference Adapter:** *(record in `T00a`)*
* **Physical Android Device / Driver:** *(record in `T11`)*
* **Baseline CPU Screenshots:** *(record exact CLI commands & paths in `T00a`)*

---

## Session Log

*(Append a brief entry after each completed task: task ID, commands run, pass/fail summary, and any contract adjustments.)*