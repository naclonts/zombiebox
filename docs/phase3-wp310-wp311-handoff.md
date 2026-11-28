# Phase 3 · WP 3.10 and 3.11 handoff

Generator v4 is the default for action play and the field atlas. A seed still identifies a repeatable 512 × 512 county and accepts the full unsigned 64-bit range. Version 4 can change roads, lot selection, and world coordinates for an existing seed. The version is published in the atlas payload. Active runs continue to use the world they started with. The current run state has no persistent save migration associated with this version.

Default counties include the five furnished house families, a grocery, and a gas compound with separate mart and canopy structures. Hospital, police, downtown, and cabins continue through existing generation. Army Base parcels, landmarks, and runtime materialization were retired. Individual `CountyFixture` choices still use the authoring geography, and `CountyFixture::None` explicitly selects the old geography.

The planner records the requested and selected town shape, junctions at shared centerline samples, road classes, turnaround ownership, and bridge spans. Crossroads adds a second arterial, split-bank uses a validated dry approach and bridge to a residential parcel across the river, and lakeshore follows the dry shore setback to another residential parcel. If a required route, crossing, or destination parcel cannot fit, the plan keeps the ribbon and records the reason. Retail prefers a planned commercial frontage. A constrained plan can use an existing city lot, with the remaining downtown families reassigned to preserve the roster.

## Review seeds

| Seed | Town shape | County start |
| --- | --- | --- |
| `7` | Highway ribbon | `(228, 285)` |
| `1` | Crossroads | `(221, 256)` |
| `2` | Riverfront split-bank | `(286, 177)` |
| `0` | Lakeshore | `(229, 176)` |

Seed `24` exercises a fallback. Seed `2`'s surveyed bridge spans `(160, 227)` to `(145, 227)`. Both banks and the intervening deck were traversed in the action runtime on foot and by motorcycle. The motorcycle stayed mounted and retained full condition. Seed `42` remains the general playthrough reference. Its v4 start is `(283, 285)`.

Seed `42`'s Craftsman, Ranch, and Colonial entrances are `(265, 292)`, `(291, 262)`, and `(284, 236)`. The grocery's public approach is `(211, 230)`, and its door is `(211, 228)`. The gas compound's public approach is `(267, 247)`, and the mart door is `(266, 261)`.

## Verification

The clean `387497d` workspace baseline reproduced five failures. Both doorway sweeps treated an overhead garage opening as a single ordinary door, and the Ranch interaction did not open the whole opening. The Craftsman persistence test expected the damaged tile to disappear when it left the active window even though the runtime kept the materialized chunk. The renderer seam test used a minimum count of vertices with identical material tags on both sides of the boundary. The seed 42 terrain seam and diagonal raycast check passed at baseline. The updated renderer seam test checks every base grid corner on both sides at zero ULP and retains the default overlay equality assertions.

The county checks replay roads, parcels, and placements, walk the grocery's public and service access, exercise the gas forecourt in both directions, check live Read and a breached wall on revisit, and sweep regular and garage openings through the streamed county. Blueprint prop and readable registration now retries unavailable anchors independently so a deferred parcel does not delay other furnishings. The existing container regression verifies that opening a Craftsman fridge leaves the terrain revision unchanged.

The optimized core suite passed all 141 unit tests, including the bilateral doorway sweep over seeds `0..63`. The terrain seam and diagonal raycast check also passed after cutover. Full workspace verification after the last retail fallback change and the extended 320-seed roster rerun were still pending at handoff. The earlier roster audit isolated a missing grocery and gas site on separate seeds. Both seeds are now included in the focused roster check.

Chrome 157 displayed all four morphology seeds and seed `42` in the atlas. Seed `42` reproduced the same survey. The browser walks reached `(275, 277)` from the seed `42` spawn and `(280, 171)` from the seed `2` spawn. Isometric and Chase views were captured. Automated action tests covered entrances, stairs, cutaways, Read, and retail access separately from those browser walks.

Host `check3d` passed with Mesa llvmpipe and captured Birch Street outdoors, breach and upstairs, plus seed `42` residential spawn, downtown, hospital, cabin, and bridge. `preview3d` captured seeds `42` and `2` in Isometric and Chase. The 180-frame downtown movement sample averaged 212.2 ms and peaked at 518.1 ms in offscreen CPU rendering with readback. These measurements describe the host harness. The physical Android 60 Hz frame-pacing check, sub-0.35 ms dirty-chunk prop rebuild target, and per-chunk triangle and draw budgets remain open. The C host ABI smoke and ARM64 Android Rust check passed. The physical handset run and Birch Android Read are pending.

Local capture files are under `/tmp/zombiebox-wp311-review`. Start with `town-atlas.png` and `browser-playtest.png`.
