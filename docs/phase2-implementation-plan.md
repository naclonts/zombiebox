# Phase 2 implementation plan

Phase 2 provides reusable props, searchable furniture, signage, and the `Read` interaction. See the [living buildings roadmap](living-buildings-plan.md#4-phased-implementation-roadmap) and [prop asset catalog](prop-asset-catalog.md) for rollout and source choices.

## Shared prop and mesh contract

- Core owns stable `PropId` values and `PropDef` gameplay data (footprint, collision bounds, cover, attachment layer, durability, and loot). Rendering resolves a prop ID and state to a shared prototype. Furnishing and gameplay tasks consume these interfaces and route mesh requests to the asset tasks.
- Convert the selected GLBs and kernelCAD inputs offline. Keep the conversion inputs, manifest, and indexed prototypes together under `assets/living-buildings/` in the repository. Normal game builds and runtime should consume the checked-in meshes without Python, kernelCAD, or a model loader. [Rebuild instructions](#offline-prototype-conversion)
- Store baked prototype positions, unit outward normals, RGBA vertex colors, and triangle indices. Normalize to meters with a bottom-center pivot and canonical simulation facing +Y. Core coordinates are Z-up. The renderer maps `(x, y, z)` to `(x, z, y)`, so render up is +Y and forward is +Z. The converted files already store renderer coordinates and CCW winding. Place wall and ceiling meshes relative to explicit attachment offsets.
- Convert flat materials and palette textures to vertex color, splitting vertices at hard normals and material boundaries as needed. Map approved materials to the game palette, preserve informative color contrasts, and assign emissive tags to approved screen surfaces. Inspect detailed textures and translucent surfaces on conversion. Use geometry or an opaque material when it reads well with the current renderer. Runtime UVs are unnecessary.
- Record the creator, source URL, source filename or archive member, version or hash, license, conversion settings, output ID, and state in the manifest. Store measured dimensions in simulation XYZ and check fit against `PropDef`. Compare variant pivots, support footprint, and interaction clearance. Assign per-instance room, floor, and cutaway metadata when placing prototypes.
- Start with review targets of 50–300 triangles per small prop, 100–600 per common furnishing, and up to 1,200 per complex item. Review high-count models in repeated placements against the chunk budget. The catalog records source counts where measured. Conversion can change those counts.

## Offline prototype conversion

Run the converter from the repository root with Python 3.11 and the kernelCAD 0.15.0 CLI on `PATH`. The CLI runs on Node.js 22. The converter uses `assets/living-buildings/` by default. Use `--source` to read a different source folder, `--out` to write a separate review build, or `--kernelcad` to pass the CLI executable or its Node command and script path.

```sh
python -m pip install numpy==1.26.4 scipy==1.15.3 trimesh==4.8.3 Pillow==11.3.0 fast-simplification==0.1.13
python scripts/convert_living_building_props.py
```

The script writes `converted/manifest.json` and one `.mesh` per prototype or state. Format version 1 starts with four bytes `ZBPM`, a little-endian `u32` vertex count, and a little-endian `u32` index count. Each 28-byte vertex stores three `f32` position values, three `f32` normal values, and four normalized RGBA bytes. The vertices are followed by little-endian `u16` triangle indices in CCW order. Add per-instance metadata when constructing the render buffers. The manifest reports measured bounds in simulation XYZ meters.

The conversion yields 34 prototypes including four opened/closed pairs. The parked car dropped from 2,954 to 1,153 triangles and the cruiser from 3,232 to 1,154. The cart still has 2,238 triangles and needs a mobile placement review. Material colors are softened and baked opaque. Palette slots and emissive flags remain part of renderer integration.

Two fresh builds produced byte-identical meshes and manifests for all 34 prototypes. Offline checks covered every recorded source and output hash, vertex and index layout, nondegenerate triangles, winding against normals, measured bounds, and support heights. Isometric previews covered the baked shapes and container states. Chrome 156 gallery review exercised the cards across all five camera modes, including bounds and palette toggles. The gallery writes its review artifact to `target/prop-gallery.html`; generation remains gated until the review scene is approved. Browser Birch/county Read checks and the optimized host benchmark pass; collider fit, cutaways, furnished live-scene QA, Android county/deferred Birch Read, and handset measurements remain open.

Renderer evidence now includes `cargo test -p zombiebox-render3d --features gpu` (89 library and integration tests), plus the headless `check3d` Vulkan/llvmpipe run: 18,482–86,793 triangles, 15–42 instances, and 8–11 of 25 chunks in the exercised scenes. These host results do not close furnished live-scene QA, Android county/deferred Birch Read, or handset review.

Integration evidence adds a passing EventDriven ownership test: a seam-crossing prop insertion and direct destruction each dirty exactly the anchor-owner chunk, while opening a container dirties zero chunks and persists across a revisit. The three `living_props` tests pass. Browser Birch and county seed `42` harnesses passed authoritative sign open/close and immediate reread. The optimized furnished-fixture benchmark measured 6 fixtures / 1,278 triangles at p50 36µs and p95 39µs, and 32 fixtures / 4,138 triangles at p50 102µs and p95 114µs. Android release compilation passed; a responsive lower-resolution emulator reached Birch gameplay but did not surface Read, so Birch Android is deferred, county Android and handset review remain open.

The latest locked offline workspace run remains blocked by the baseline `zombiebox-core` spatial regression `county_seed_42_terrain_seam_and_diagonal_raycasts` (`hit_z=5.28868`, expected `4.1443267`). Core reproduced the same failure from a pristine `git archive HEAD`; it is not attributed to Phase 2. County integration (4/4) and core action (19/19) targeted suites pass.

Read verification (2026-11-27): headless Chrome passed Birch Street and county seed `42` Read flows. Each flow opened the authoritative title/body from the web endpoint, dismissed the modal, and Birch reopened the same sign. Android release compilation passed. A responsive lower-resolution SwiftShader emulator reached Birch gameplay, but the native HUD did not surface the Read prompt; per current prioritization, Birch Android Read verification is deferred. Android county Read, physical-handset pacing, and gallery approval remain open.

## Work packages

| Work | Owner and handoff | Depends on | Acceptance |
| --- | --- | --- | --- |
| 2A. Prop interfaces | Core registry and scene snapshot IDs, facing, layer, and state | — | Shared fixture defines a multi-tile prop and a stacked attachment. |
| 2B. Offline conversion | Converter under `scripts/` and asset manifest under `assets/living-buildings/converted/` | Coordinate and mesh contract | Convert the shortlist including the sofa, single bed, toilet, fridge, and desk. Repeat the build and compare output. Check geometry and provenance. |
| 2C. Residential and office | Furniture prototypes in the converted asset folder and palette bindings in the renderer | 2B | Beds, seating, tables, kitchen and bath fixtures, TV, and desks match registry dimensions and read at gameplay zoom. |
| 2D. Retail | Checkout lane and retail shelving modules in `mesh/kit.rs`, plus vetted retail dressing | 2A, 2B | Vary lanes and shelf lengths and reuse the shelf profile for pharmacies. |
| 2E. Medical and exterior | Audit and convert approved medical equipment, storage, and static parked vehicles | 2B | Check medical silhouettes and pivot fit. Hand vetted clinical assets to Phase 4 placement. |
| 2F. Gameplay variants | Procedural gaps, open container variants, and screen materials alongside the related prototypes | 2A, 2B | Base and opened states align and retain a clear interaction approach. |
| 2G. Footprints and destruction | Core prop spans, cover, collision, damage routing, and chunk invalidation | 2A | Test rotations, seam-spanning anchors, clearance, and damage through any span tile. |
| 2H. Static rendering | Renderer mesh library and chunk integration | 2A, 2B | Parts retain room visibility, correct floor ownership, and cutaway behavior. |
| 2I. Container instances | Renderer dynamic state batches | 2G, 2H | One search switches state and preserves it across streaming with zero chunk rebuilds. |
| 2J. Sign geometry | Renderer pixel relief and attachment rendering | 2H | Wayfinding remains legible in reference views. |
| 2K. Read interaction | Core prompt and browser/Android text modal | 2A | Signs, directories, notes, and journals supply their selected text to the frontend. |
| 2L. Gallery and integration | Renderer example and reference scene verification | 2C–2K | Exercise the completion checks below. |

Agree on the shared interfaces before distributing implementation. Source downloads and license checks can run alongside gameplay design. Coordinate changes to the asset manifest and shared prototype interfaces with their owners.

## Completion checks

- Add `cargo run -p zombiebox-render3d --example prop_gallery` as an approval scene before generation uses the props. The current review output is `target/prop-gallery.html`. Group prototypes by category and show every cardinal orientation, closed and opened states side by side, toggled gameplay bounds, IDs and triangle counts, existing camera modes, and residential, clinical, and distressed palettes. Generation remains gated pending approval of this scene.
- Rebuild from the repository's asset folder and compare the meshes and manifest byte for byte. Verify that every manifest source is present and matches its recorded hash.
- Check converted mesh provenance and dimensions, indices, normals, winding, palette, and variant alignment. Confirm the mesh stays inside its declared footprint or attachment envelope, including any intentional door or drawer travel.
- In Birch Street and county seed `42`, check doorway clearance, stairs, breaches, cutaways, and room shroud across cameras. Cross chunk seams and revisit. Check cover and line of sight at shelf runs and the persistence of looted and destroyed props. Verify the `Read` modal in browser and Android.
- Measure static prop rebuilds against the [prop pipeline budgets](building-assets-and-ecology.md#4-asset-creation-pipeline--60-hz-mobile-budgets) and confirm zero chunk rebuilds on container search. Record frame pacing in a furnished residential block and a dense interior on a modest physical Android handset, with host, emulator, and handset results recorded separately.
