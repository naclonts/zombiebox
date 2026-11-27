# Zombiebox — motorcycle paint and condition variants

## 1. Goal and scope

- Starter motorcycle (vehicle slot 0): always the original `GreenBlack` palette, pristine cosmetic age, mechanical condition 100 at spawn. It may acquire visible damage through normal riding/crashes afterward.
- Every other motorcycle drop: seeded random paint and cosmetic condition, fixed for that vehicle across frames, chunk streaming, mounting and dismounting. Repeats are allowed. Same world seed produces the same appearances; a different seed generally produces different appearances.
- Cosmetic age is independent of the existing mechanical `condition: u8`. Do not make an old-looking drop less operable, and do not change spawn placement, physics, damage, HUD or collision behavior.
- Use the current articulated mesh, its four parts, pivots and rider locators. No new GLBs, textures, shader changes, model geometry, FFI fields or dependencies needed.

## 2. Current architecture (verified)

| File | Relevant behavior |
|---|---|
| `crates/zombiebox-core/src/world/vehicles.rs` | `MOTORCYCLE_SLOTS = 4`; starter slot 0, deterministic roadside roster |
| `crates/zombiebox-core/src/action/vehicles/mod.rs` | `vehicle_id(seed, slot)` encodes slot in low byte and seed in remaining bits; `Vehicles::from_spawns` creates parked bikes; `MotorcycleView` includes condition |
| `crates/zombiebox-core/src/action/vehicles/motorcycle.rs` | `MotorcycleState::parked` sets condition 100; crashes and collisions subtract condition; zero disables powered motion |
| `crates/zombiebox-render3d/src/scene/mod.rs` | Copies `(VehicleId, VehicleView)` into dynamic frame snapshot |
| `crates/zombiebox-render3d/src/mesh/motorcycle.rs` | Five paint palettes, `Face = (u32,u32,u32,u8)`, semantic material index, four mesh parts, vertex-color triangles |
| `crates/zombiebox-render3d/src/mesh/vehicles.rs` | Existing transforms, wheel pivots, lean, steering and `default_motorcycle_paint()` |
| `crates/zombiebox-render3d/src/presentation.rs` | Registers four green/black prototypes; all bikes share them; current tipped/zero-condition tint is 0.62 RGB |
| `crates/zombiebox-render3d/src/gpu/pipeline.rs` | Existing `register_prototype` and `update_prototype` methods; prototypes are retained in a Vec |

Implement appearance in the renderer only. `VehicleId` already contains a stable seed-derived identity; don't introduce fields into core or change the world RNG. The existing `zombiebox_core::mix64` is public and deterministic. If repainting or washing is implemented later, move mutable appearance state into core at that time.

## 3. Palette registry

Add `crates/zombiebox-render3d/src/mesh/motorcycle_appearance.rs` and declare the module in `mesh/mod.rs`. Move the existing `MotorcyclePaint` enum and its `palette()` method into it and re-export `pub use super::motorcycle_appearance::MotorcyclePaint;` from `motorcycle.rs` to keep existing imports compiling. Derive `Clone, Copy, Debug, Default, Eq, PartialEq, Hash`; default stays GreenBlack. Add `pub const ALL: [Self; 22]` and `pub const fn palette(self) -> [[u8; 3]; 3]`.

Each row below is an enum variant and three RGB24 values: primary, secondary, pinstripe. Keep the first five palettes **exactly** as in the current source. The remaining values are artistic period-inspired/custom colors, not calibrated OEM Honda paint codes. Choose uniformly from ALL for non-starter bikes.

| Enum variant | Primary | Secondary | Pinstripe |
|---|---|---|---|
| GreenBlack | #265B43 | #181D1E | #AC9052 |
| BurgundyBlack | #71232B | #191B1D | #B18F4F |
| CreamBlack | #CCBE9A | #1B1F1F | #97753C |
| MidnightBlue | #294569 | #161B21 | #97A6B1 |
| MatteBlack | #313333 | #141617 | #696967 |
| SomersetOrangeWhite | #C36331 | #EEE4CF | #A47D40 |
| SonomaGreenWhite | #285D47 | #EAE1CC | #B59658 |
| SedonaRedSilver | #8E2936 | #92969B | #D0D2CF |
| BeringBlueBlack | #284B7B | #191B1D | #B5BEC7 |
| BlazeYellowBlack | #D9AE24 | #191B1D | #A47C36 |
| ChaparralBeigeBlack | #C0AD8B | #191B1D | #8B7046 |
| AmericanRedBlack | #B62927 | #191B1D | #C4B48B |
| OrionSilverWhite | #AEB5BF | #EEECE4 | #777E89 |
| RoyalPurpleSilver | #573477 | #A6A8AF | #D6D5C9 |
| SunburstOrangeBlack | #D37126 | #191B1D | #C5A26A |
| MilitarySurplus | #62694A | #262925 | #777C63 |
| RustDevil | #75332D | #242220 | #98704F |
| Route66 | #418783 | #E5D4B2 | #BBA16D |
| Copperhead | #A26D45 | #272323 | #D4AA71 |
| GhostRider | #D9D7D0 | #26282B | #82868A |
| BlackCherry | #72324B | #201E24 | #BCA071 |
| Wasteland | #B3A17B | #665A44 | #8C7954 |

Use a `const fn rgb(hex: u32) -> [u8; 3] { [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8] }` to make palette definitions easy to edit. Keep fixed chrome, rubber and seat colors unchanged for pristine paint variants. Names such as MilitarySurplus and Copperhead refer only to paint in this first implementation; optional matching seat/metal treatments can be added independently later.

## 4. Deterministic appearance and damage

In `motorcycle_appearance.rs`:

```rust
use zombiebox_core::{VehicleId, mix64};

pub const MOTORCYCLE_SLOT_COUNT: usize = 4;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CosmeticAge {
    #[default] Pristine,
    Dusty,
    Weathered,
    Rusted,
    BattleDamaged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MotorcycleAppearance {
    pub paint: MotorcyclePaint,
    pub age: CosmeticAge,
    pub pattern_seed: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MotorcycleVisualKey {
    pub appearance: MotorcycleAppearance,
    pub damage_bucket: u8, // 0..=5
}

pub fn motorcycle_slot(id: VehicleId) -> Option<usize> {
    let slot = (id.0 & 0xff) as usize;
    (slot < MOTORCYCLE_SLOT_COUNT).then_some(slot)
}

pub fn appearance_for(id: VehicleId) -> MotorcycleAppearance {
    let pattern_seed = mix64(id.0 ^ 0x5041_5454_4552_4e31);
    if motorcycle_slot(id) == Some(0) {
        return MotorcycleAppearance {
            paint: MotorcyclePaint::GreenBlack,
            age: CosmeticAge::Pristine,
            pattern_seed,
        };
    }
    let p = mix64(id.0 ^ 0x5041_494e_545f_3031);
    let a = mix64(id.0 ^ 0x4147_455f_3030_3031) % 100;
    MotorcycleAppearance {
        paint: MotorcyclePaint::ALL[(p % MotorcyclePaint::ALL.len() as u64) as usize],
        age: match a {
            0..=9 => CosmeticAge::Pristine,
            10..=39 => CosmeticAge::Dusty,
            40..=69 => CosmeticAge::Weathered,
            70..=89 => CosmeticAge::Rusted,
            _ => CosmeticAge::BattleDamaged,
        },
        pattern_seed,
    }
}

pub fn visual_key(id: VehicleId, condition: u8) -> MotorcycleVisualKey {
    MotorcycleVisualKey {
        appearance: appearance_for(id),
        damage_bucket: (100 - condition.min(100)).div_ceil(20),
    }
}
```

Age probabilities for non-starters: 10% pristine, 30% dusty, 30% weathered, 20% rusted, 10% battle-damaged. These are tunable design defaults. Use separate domain hashes for paint, age and pattern. Never use frame time, position, chunk or RNG iteration order. The starter paint remains green/black forever; only runtime damage affects its look. Bucket changes at condition 99, 79, 59, 39, 19; cap at 5. Damage bucket 0 means no mechanical damage. No cosmetic randomization in core.

## 5. Material-aware weathering

Current generator assigns semantic IDs; preserve these (confirmed against the original `generate_motorcycle_mesh.py`, included in the asset pack):

| ID | Meaning | Aging response |
|---|---|---|
| 0 | primary paint | fade, dust, rust, chips |
| 1 | secondary paint | fade, dust, rust, chips |
| 2 | stripe | fade, dust, chips; little/no rust |
| 3 | black trim | dust, fading |
| 4 | rubber | dust, slight fading; no rust |
| 5 | frame | dust, rust, chips |
| 6 | chrome | tarnish, dust, small rust spots, scratches |
| 7 | chrome highlight | tarnish, dust, small rust spots, scratches |
| 8 | steel | tarnish, dust, rust, scratches |
| 9 | dark metal / radiator | dust, slight rust |
| 10 | lamp / plate | preserve color and readability |
| 11 | red lens | preserve |
| 12 | amber lens | preserve |
| 13 | seat | dust, slight fading; no rust |
| 14 | engine fins | tarnish, dust, small rust spots |

### Public mesh interface

In `mesh/motorcycle.rs`, retain the existing `motorcycle_part_mesh(part, paint)` and `motorcycle_mesh(paint)` API, preserving their pristine output byte-for-byte. Add:

```rust
pub fn motorcycle_part_mesh_with_appearance(
    part: MotorcyclePart,
    visual: MotorcycleVisualKey,
) -> MeshBuffer;
```

Have both mesh entry points use the same face loop. The old API may call the new one with pristine age, pattern seed 0 and damage bucket 0. In that loop derive the base RGB exactly as today (`palette[m]` for m<3, `FIXED[m-3]` otherwise). Convert triangle centroid from part-local to model-space using `motorcycle_part_pivot(part)`; use unscaled model meters, not the live wheel or steering transforms. Feed base RGB, material ID, model-space centroid and visual key to the color function:

```rust
pub fn weathered_rgb(base: [u8; 3], material: u8,
                     model_pos: glam::Vec3, visual: MotorcycleVisualKey) -> [u8; 3];
```

Keep geometry, vertex order, normals, alpha=255 and `GeometryTag::Prop` unchanged. Do not add triangles. Early-return `base` when age is pristine AND damage bucket is zero, and for material IDs 10..=12. The existing shader applies lighting and fog already; weathering is only a base-color adjustment.

### Algorithm (implement exactly first; tune after visual review)

Use these age coefficients (`fade, dust, rust, tarnish, chips`), all normalized 0..1:

| Age | Fade | Dust | Rust | Tarnish | Chips |
|---|---:|---:|---:|---:|---:|
| Pristine | 0 | 0 | 0 | 0 | 0 |
| Dusty | .06 | .48 | .00 | .06 | .00 |
| Weathered | .24 | .35 | .14 | .34 | .08 |
| Rusted | .30 | .45 | .67 | .60 | .14 |
| BattleDamaged | .18 | .30 | .28 | .35 | .65 |

Material multipliers in the same order:

| IDs | Fade | Dust | Rust | Tarnish | Chips |
|---|---:|---:|---:|---:|---:|
| 0,1 | 1 | 1 | .80 | 0 | 1 |
| 2 | .80 | .80 | 0 | 0 | .40 |
| 3 | .55 | .85 | 0 | 0 | 0 |
| 4 | .30 | 1.15 | 0 | 0 | 0 |
| 5 | .20 | 1 | 1 | .10 | .90 |
| 6,7 | 0 | .60 | .30 | 1 | .45 |
| 8 | 0 | .85 | .85 | .80 | .65 |
| 9 | 0 | 1 | .45 | .30 | .25 |
| 13 | .55 | .75 | 0 | 0 | 0 |
| 14 | 0 | .80 | .35 | .70 | .25 |

Let `d = min(damage_bucket, 5) / 5.0`. Increase age chips by `0.75*d` (clamp 0..1), dust by `0.12*d`, tarnish by `0.12*d`. Leave rust/fade age-driven (crashing does not instantly rust a bike). Multiply these coefficients by material weights. Don't use phase as a damage input: tipping is a pose, not a permanent change of color.

**Noise:** implement seeded 3D value noise `noise3(seed, p) -> f32` (0..1). Floor coordinates to signed integer cells; hash each of 8 lattice corners with `mix64(seed ^ mix64(x as u64) ^ mix64((y as u64).rotate_left(21)) ^ mix64((z as u64).rotate_left(42)))`, convert top 24 bits to a unit float by dividing by 16777216, trilinearly interpolate with smoothstep `t*t*(3-2*t)` on fractional coordinates. Do not hash face index (salt-and-pepper triangles), use world position, or use a nondeterministic RNG. Use separate seeds below. Define `smooth(lo,hi,x)` as the same cubic smoothstep of clamped `(x-lo)/(hi-lo)`.

Color processing in the renderer's current RGB/255 space, using `glam::Vec3` or arrays; `mix(a,b,t) = a*(1-t)+b*t` with t clamped:

```text
p = model-space centroid (meters)
s = visual.appearance.pattern_seed
low = 1 - smooth(0.12, 0.85, p.y)
patch = noise3(s ^ 0xA11CE001, p * 9)
fine = noise3(s ^ 0xA11CE002, p * 28)
streak = noise3(s ^ 0xA11CE003, p * vec3(7, 40, 12))

c = base / 255
luma = dot(c, [0.299, 0.587, 0.114])
c = mix(c, vec3(luma), fade * 0.55)
c = mix(c, [0.62,0.61,0.56], fade * 0.18)

# Tarnished chrome/steel (does not affect paint or rubber)
c = mix(c, [0.32,0.34,0.33], tarnish * (0.40 + 0.60*patch))

# Rust clusters; more concentrated near chassis and lower fenders
rust_field = 0.75*patch + 0.25*low
rust_mask = smooth(0.54, 0.76, rust_field)
rust_color = mix([0.26,0.12,0.065], [0.52,0.25,0.10], fine)
c = mix(c, rust_color, rust * rust_mask)

# Dirt accumulation, more at tires, frame and lower surfaces
dust_mask = (0.25 + 0.75*low) * (0.50 + 0.50*patch)
c = mix(c, [0.49,0.43,0.33], dust * dust_mask)

# Scrapes / chips: a few elongated clusters, not per-frame flicker
chip_field = 0.65*streak + 0.35*fine
chip_mask = smooth(0.57, 0.79, chip_field)
chip_color = [0.34,0.36,0.35] for m in 0,1,2,5,9
             [0.67,0.68,0.65] for m in 6,7,8,14
             otherwise c
c = mix(c, chip_color, chips * chip_mask)
return round(clamp(c, 0, 1) * 255) as RGB8
```

Keep colors opaque and recognizable. This is stylized vertex-color weathering, not physically based corrosion or actual dent geometry. At current triangle resolution, tiny scratches may appear as broad faceted patches; review in first/third person. Do not convert only motorcycles to a new color space or modify the global shader. All damage masks remain stationary relative to the mesh even while wheels spin.

## 6. GPU prototype cache and presentation

The engine currently uploads only four green/black parts in `Presentation::new`, and `prepare`/`batches` share those prototypes. A per-instance RGB tint is insufficient (it would recolor the chrome, lamps, tires, seat and both paint regions together). Do not register all 22*5*6 possible variants, and don't upload a mesh every render frame.

Use **four reusable vehicle slots**, each with four part prototype IDs (max 16 motorcycle GPU prototypes total). Leverage existing `GpuRenderer::update_prototype`, which replaces GPU buffers without growing its prototype Vec. No GPU API changes needed.

In `presentation.rs`:

```rust
const MOTORCYCLE_PARTS: [MotorcyclePart; 4] = [
    MotorcyclePart::Body, MotorcyclePart::Steering,
    MotorcyclePart::FrontWheel, MotorcyclePart::RearWheel,
];
#[derive(Clone, Copy)]
struct MotorcyclePrototypeSet {
    key: MotorcycleVisualKey,
    ids: [PrototypeMeshId; 4],
}
// Presentation fields:
// motorcycle_prototypes: [Option<MotorcyclePrototypeSet>; MOTORCYCLE_SLOT_COUNT]
const MOTORCYCLE_BATCH_COUNT: usize = MOTORCYCLE_SLOT_COUNT * MOTORCYCLE_PARTS.len();
// DOOR_BATCH and BATCH_COUNT continue to derive from the above.
```

1. In `new`, replace four eager default registrations with `[None; MOTORCYCLE_SLOT_COUNT]`. All actor/cache/door prototypes stay unchanged.
2. Add `ensure_motorcycle_prototypes(&mut self, ctx: &GpuContext, slot: usize, key: MotorcycleVisualKey)` (or a helper taking disjoint field borrows). If existing key matches, return immediately. On first use build all four part meshes and call `register_prototype`, saving their IDs. On changed key build all four and call existing `update_prototype` with the same IDs. Store the new key after uploads. Labels can be `motorcycle_{slot}_{part:?}`. Validate generated meshes in tests; `update_prototype` is currently best-effort on valid nonempty meshes.
3. In `prepare`, iterate snapshot `(id, view)` as now. Resolve slot via `motorcycle_slot`, skip invalid slot defensively (debug assert for unexpected roster IDs). Compute `visual_key(*id, moto.condition)`, then ensure its prototypes. Preserve existing wheel phase math, steering/lean transforms, rider animation and camera behavior. Use `vehicle_base + slot * 4 + part_index` for the instance index. Remove the current tipped/zero-condition `.with_tint([0.62, ...])` branch; condition-based color already represents damage and phase must not flicker the paint.
4. In `batches`, accept the new array instead of `[PrototypeMeshId;4]`; loop over populated sets, and four part IDs in each. Map each to the exact same batch offset as step 3. Skip `None` and empty motorcycle batches. Keep actor, cache, door and effects ordering unchanged. Both `render_offscreen` and `render_to_surface` must use the updated helper.
5. In `reset`, clear wheel phases as today but **retain** the prototype slots and their keys/IDs. The renderer retains its prototype Vec; clearing this cache and registering again would leak prototype slots across repeated games. A new world will update mismatched keys on demand. Do not remove cache records when a bike leaves the streaming bounds. The pattern seed is in the key, so new runs with different patterns cannot reuse the wrong baked colors.

Current roster has 4 bikes max. The mesh has approximately 15k triangles, and the face loop emits three vertices per triangle (48-byte vertex + 4-byte index). Four complete uploaded bikes are roughly 9 MB of raw buffers before driver overhead. No per-frame weathering CPU work or additional draw calls compared with drawing four distinct bikes separately; each visible bike takes up to four part draw calls. No all-palette GPU prewarming. If roster size grows significantly, revisit using shader material IDs and per-instance palette/weathering parameters.

## 7. Task slices (sequential commits; can hand each to a small agent)

**A — Appearance registry and deterministic selection.** Create `motorcycle_appearance.rs`, move/re-export enum, implement palette table and `appearance_for`/`visual_key`. Preserve all existing APIs. Test exact original palette values, all 22 unique variants, slot 0 invariants for many seeds, deterministic non-starter sampling and damage buckets. No rendering changes yet.

**B — Material-aware mesh recoloring.** Implement `weathered_rgb` and noise, add new mesh function. Preserve existing geometry and pristine output. Tests below. Do not touch the enormous generated triangle arrays. Header currently has a generator comment; the original script is shipped in `docs/zombiebox_motorcycle_pack.zip` rather than a standalone tracked script. Document the hand-written appearance module as the canonical runtime palette; do not overwrite the current mesh header/locators by blindly rerunning the archived generator. If generator reproducibility is addressed in this task, adapt its emitted wrapper to delegate to the new module while preserving current rider locators.

**C — Presentation integration.** Implement four-slot prototype cache and batching changes. Exercise both render paths and restart/streaming. Retain old prototype IDs across resets. No changes to core, motion, FFI or shaders.

**D — Review and regression tests.** Add a headless/offscreen visual preview gallery if useful, using existing render3d examples and lighting. Show pristine green/black and identical paint in each age, several alternate paint palettes, and one starter after damage. Test daylight and dusk, close-up and isometric. Tune coefficients only if necessary.

## 8. Acceptance tests

- `appearance_for` for slot zero across many generated IDs: always GreenBlack and Pristine. At condition 100 mesh colors exactly match original. Real condition loss changes damage bucket and visible wear while preserving paint and cosmetic age.
- Test sample non-starter IDs across at least 256 seeds, not merely three bikes in one seed: expect diverse paints and all age categories, deterministic repeat results; don't assert every seed produces unique colors. Test appearance remains constant across all condition values.
- Test damage buckets at 100, 99, 80, 79, 60, 59, 40, 39, 20, 19, 0, and input >100.
- Assert pristine appearance returns exact input RGB for all 15 material IDs. Lenses unchanged for all ages and damage buckets. Rubber and leather never acquire rust or exposed-metal chips; verify material weights directly or with isolated coefficient tests. Rusted paint differs from pristine at some actual face samples. Colors deterministic and finite/opaque.
- For each of 22 paints and each of five ages (and representative damage buckets 0, 1, 5), check every generated part with `MeshBuffer::validate()`. Compare positions, normals, indices, metadata and AABBs with original meshes; only color may change. If exhaustive test is slow in debug, run matrix in release or split into representative full mesh tests and exhaustive per-color tests.
- Check prototype cache behavior: first visible bike allocates exactly four IDs; subsequent identical key reuses them; different damage bucket updates same IDs; different bike slot gets a distinct set; reset and new world reuse existing slot IDs; offscreen/reentry doesn't reallocate. With all four slots present, never exceed 16 motorcycle prototypes. Confirm door batch offset does not overlap vehicle batches.
- Existing ride, crash, rider locator and camera tests pass. No change in `MotorcycleState` or starter placement/condition. Test that a starter bike after a crash does not revert to pristine on chunk reentry; non-starter weathering remains fixed through chunk streaming and mounting.

Run from repo root:

```sh
cargo fmt --all -- --check
cargo test -p zombiebox-core
cargo test -p zombiebox-render3d
cargo check --workspace
```

Where supported, run existing native/mobile/web builds and smoke tests. Headless GPU tests may skip if no adapter; report this explicitly. Do not claim visual or platform validation unless actually performed.

## 9. Done when

The initial starter always looks like the original pristine green/black asset. Each of the remaining three possible county drops has a stable, randomly selected paint and independent cosmetic age; crash wear reacts to existing mechanical condition. Both render paths use correct animated parts. No per-frame mesh reconstruction, unbounded GPU growth, new assets, changes to simulation semantics, or regressions to existing vehicle controls.
