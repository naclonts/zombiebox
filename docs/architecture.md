# Zombiebox architecture

## Stack decision

The first local Android app uses React Native for the HUD, menus, and action controls, with a native landscape view for the game. The Android host calls a separate float-position action runtime in `zombiebox-core` and the sibling software renderer through JNI. Rust renders into the pixels of the Android bitmap. The Rust simulation owns gameplay and fixed-step timing, while the Android view owns touch sampling and presentation. [React Native performance](https://reactnative.dev/docs/performance)

The Android path is the current native target. iOS remains a future host: its view adapter, presentation path, and device build have not been implemented. If placing doors, walls, and occluders becomes the bottleneck, run the same street in Godot's tilemap editor and compare the time from layout edit to playable build. The headless rules and replay fixtures give both experiments a common reference. [Godot tilemaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html)

## Runtime ownership

| Owner | Responsibility | Across the boundary |
| --- | --- | --- |
| React Native shell | HUD, loading, pause, settings, hotbar, and overlay controls | Display status from the native host; send pause, settings, tool, and action input. |
| Android host | Run lifetime, focus, touch sampling, frame callbacks, audio/haptics playback, bitmap presentation | Own the run handle and call JNI with each frame and sampled controls; drive `AudioTrack` SFX and haptics from simulation events. |
| Rust action runtime | Float player/zombie/vehicle poses, collision, aiming, weapons, destructible 2.5D tiles, AI, procedural world chunks | Advance the simulation at 60 ticks per second and expose poses, events, and HUD state to Rust rendering and JNI. |
| Rust software renderer | Isometric world (and future vehicle camera), multi-level cutaways, lighting, actors, vehicles, and transient effects | Render current action state into a packed RGBA framebuffer for Android bitmap presentation. |
| Legacy browser host | Existing 20 Hz tile-command encounter and Canvas client | Keep existing browser inputs, HTTP transport, and deterministic command rules. |

The C ABI exposes both the legacy command game and a native action run that advances from frame timestamps, accepts normalized dual-stick inputs and commands, and renders into caller-owned pixel memory. Handles stay on the native side. Android passes a bitmap to the JNI adapter; the Rust renderer writes the RGBA framebuffer into it. [Public C header](../crates/zombiebox-ffi/include/zombiebox.h) · [Mobile component spec](../apps/mobile/specs/ZombieboxSurfaceNativeComponent.ts)

## Browser playtest

The dev host owns each `Game` in Rust and serves a Canvas client. The client drives the 20 Hz clock, sends ordered batches of up to three commands, and renders the returned tile window, actors, HUD, and tick events. The simulation advances only when a command batch arrives. This lets Android Chrome test the encounter and touch controls on the Debian host through Tailscale.

The browser host starts the residential scenario in the legacy tile-command runtime. Rust defines the playable bounds, house walls, doorway, and pistol cache. The headless example and existing C callers continue to start the original encounter. The browser uses its existing WASD/arrow, scavenge, fire, and pause controls.

The browser transport, Canvas view, and legacy rules are independent of the local Android action game. Android creates its own run from a seed and steps it locally. Both action runtime and legacy runtime live in `zombiebox-core`, while retaining separate gameplay inputs and update rates.

## One frame on a phone

1. The app starts in landscape and opens the run after **Enter the block**. The native view samples left-stick and right-stick inputs independently (transforming screen directions to world axes on foot, or mapping handlebar steering and rider lean while on a motorcycle). The thumbstick areas consume their own touches. Contextual interact, hotbar switching, quick-heal, and stick-integrated firing are consumed cleanly across simulation ticks.
2. The Android frame timestamp feeds a monotonic accumulator in the Rust action FFI. It advances at **60 ticks per second**, caps elapsed time after long frames, and retains discrete actions until a simulation tick consumes them. Pausing clears pending input and resets the accumulator.
3. After each tick, the FFI reads events before the next step clears them and informs both the renderer and the Android audio/haptics engine. The core resolves combat, destruction, AI awareness, physics, and outcomes. The renderer draws muzzle flashes, blasts, debris, lighting, and hit reactions. The HUD status packs health, ammunition, weapon/tool state, medkits, vehicle telemetry (when riding), threat count, interact prompts, and outcome for the React Native layer.
4. The software renderer draws the multi-level cutaway buildings, terrain, streetlights, dusk lighting, and directional animated sprites into a packed RGBA framebuffer. The native view presents the Android bitmap, with React Native drawing the HUD and overlays on top. [Street rendering](../crates/zombiebox-render/src/street.rs) · [Actor rendering](../crates/zombiebox-render/src/actors.rs)

On foot, the 2:1 diamond camera projects tile coordinates `(x, y)` to `((x − y) × tileWidth/2, (x + y) × tileHeight/2)`. Use projected foot positions for depth ordering and keep the player, routes, and enemies legible when walls, roofs, or effects obscure the scene. Choose assets, lighting, and rendering techniques that meet the visual target within the frame budget on modest Android hardware. [Visual and performance target](product-decisions.md#visual-and-performance-target)

## Scaling from one street to the county

Expanding from the single Birch Street residential block to a **Minecraft-style seed-driven procedural world** and **drivable vehicles** requires evolving five architectural boundaries cleanly:

- **Two-tier procedural world generation.** `World::new(seed)` first fixes the terrain raster, district bounds, connected roads, public approaches and lot frontages. Parcel template choices use stable district ordinals. Looking up a tile or requesting a 16 × 16 chunk materializes detail for the intersecting parcels and reuses it across seams. Separate hash domains control layout, outdoor details, loot and infected. `World::chunk_sites` returns intersecting buildings and the loot and infected owned by that chunk across all floors. The full atlas explicitly requests the complete manifest. [Geography](../crates/zombiebox-core/src/world/geography.rs) · [Parcel queries](../crates/zombiebox-core/src/world/parcels.rs)
- **Atlas presentation.** The browser host serves a compact row-major survey under `/api/county/<seed>`. The Android atlas Activity serves the same JSON from Rust on a WebView request thread and loads the bundled Canvas assets offline. The map reads the generated features for collision, stair transitions and cutaways. [Survey format](../crates/zombiebox-core/src/survey.rs) · [Android atlas](../apps/mobile/android/app/src/main/java/game/zombiebox/mobile/CountyActivity.kt)
- **Data-driven building and material templates.** Move beyond hardcoded house/prop coordinates in Rust toward composable, data-driven multi-floor building templates per district (residential houses, downtown storefronts/offices/apartments, multi-wing hospitals, police precincts, army base compounds, woodland cabins) backed by a shared destructible material table so every district matches Birch Street's destructibility and persists changes uniformly as chunk edit deltas.
- **Chunk streaming, spatial indexing, and stable entity IDs.** Cache visible chunks plus a margin and evict distant chunks cleanly, extending the look-ahead prefetch window along the velocity vector when riding motorcycles, driving jeeps, boating, or flying planes. Track active monsters, projectiles, and vehicles in spatial buckets with stable IDs independent of their current chunk so entities crossing chunk seams (or unmanned skidding motorcycles) never duplicate or vanish.
- **Throttled, invalidatable pathfinding & distant hordes.** Keep pathfinding scoped locally across active chunks/floors with local invalidation when doors open or windows/walls break. Model distant road hordes abstractly on the macro road graph and materialize/reconcile them into individual monsters as they enter or leave the active simulation radius.
- **Vehicle physics & camera/renderer headroom.**
  - **Vehicle simulation:** Give vehicles continuous physics state in `zombiebox-core` (for motorcycles: position, velocity, heading, speed, handlebar steer angle, body lean angle, surface traction, rider link, and unmanned skid/tumble state) running at the same 60 Hz fixed tick so wipeouts, rider ejection, skid injuries, and collisions with destructible props or infected stay deterministic and testable headlessly.
  - **Camera mode & rendering trade-off:** The current CPU software renderer (`crates/zombiebox-render`) assumes a fixed-angle 2:1 diamond isometric `Projector`. Prototyping a **behind-overhead / chase-aligned vehicle camera** (where world view rotates to follow the motorcycle's heading so left/right lean maps directly to screen left/right) introduces arbitrary view rotation and steeper draw distance at speed. Evaluate early whether a fast rotated/top-down software pass suffices for the motorcycle prototype or whether rotating cameras, water/elevation, and faster vehicle streaming justify moving rendering to GPU (`wgpu` over Vulkan/Metal) sooner. Keep audio/haptics decoupled from fixed ticks so engine pitch, tire skids, and crashes fire smoothly without stalling frames.

The software renderer lives in `crates/zombiebox-render` and depends on the core. The Android host is in `apps/mobile/android`. An iOS adapter is still future work.

## Platform and persistence

- **iOS (future).** Reuse the Rust gameplay core and renderer with an iOS host and native presentation path. No iOS adapter or device build is included yet. [CAMetalLayer](https://developer.apple.com/documentation/QuartzCore/cametallayer) · [Packaging an XCFramework](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle)
- **Android.** Build `zombiebox-ffi` for the configured ABIs and link the static Rust library plus JNI C adapter into the app's shared library. The native view presents the rendering in a bitmap. The local sideload APK uses the machine's Android debug signing key. [APK and device verification](../apps/mobile/README.md#verification) · [Android bitmap API](https://developer.android.com/reference/android/graphics/Bitmap) · [JNI tips](https://developer.android.com/training/articles/perf-jni)
- **GPU.** Prototype `wgpu` with Metal on iOS and Vulkan on Android when rotating vehicle cameras, elevation/water effects, or large-world draw distances outgrow the CPU software renderer. Confirm adapter availability on the oldest devices selected for testing and keep platform differences behind surface creation and presentation. [wgpu backends](https://wgpu.rs/)
- **Saves (planned).** At a tick boundary capture the world seed, generator and save-format versions, player inventory and position, active/parked/wrecked vehicles, consumed loot, entity state, and chunk edits. Checkpoint after meaningful changes and when backgrounded. Write a temporary save beside the prior checkpoint and atomically replace it after a successful write. Rebuild baseline chunks with their recorded generator version and overlay persisted deltas. A death checkpoint marks the run ended.

## First device decision

The headless example exercises legacy game rules, core action tests cover the native encounter, and the C smoke test checks native linking. Report APK build, emulator, and handset results separately so a successful build is not mistaken for device validation. The next platform port is iOS; it requires a host and on-device validation before it can be treated as supported.

| Exercise | Initial acceptance target |
| --- | --- |
| Move, pick up tools, aim, and shoot under the React Native HUD | Movement feels brisk and shooting integrates cleanly with twin-stick ergonomics without requiring mid-screen thumb travel. Pickup works near caches, shots respect intact walls, and ammo, health, and threat count match the Rust action runtime. Pause freezes simulation ticks. |
| Play Birch Street on a modest Android handset | Record the handset model and frame pacing during movement, aiming, and repeated shots. Check that touch controls stay responsive and that threats, doorways, and loot stay easy to read during combat. |
| Pause and resume or background and foreground the Android app | Input releases cleanly, audio suspends cleanly, and the game clock resumes without a catch-up burst. |
| Stress a street with 100 visible zombies and repeated shots | Target 60 Hz. Record p95 frame time against 16.7 ms, tick CPU cost, memory, and sustained behavior over ten minutes. These numbers are budgets to test. |
| Stream procedural chunks at motorcycle speed | Target 60 Hz without streaming hitches while riding across district and chunk seams, including camera rotation/transitions and wipeout physics. |

Next, validate the expanded Birch Street block on a physical Android handset, prototype semi-realistic motorcycles (controls, physics, and behind-overhead camera), and then layer in persistent supply runs, road hordes, and additional vehicles (jeeps, motorboats, and planes). [World design and milestones](world-design.md)
