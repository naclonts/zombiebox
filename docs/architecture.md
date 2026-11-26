# Zombiebox architecture

## Stack decision

The Android app uses React Native for the HUD, menus, and action controls. The native landscape view samples input and gives a rendering worker exclusive access to the JNI action run and its `SurfaceView`. `zombiebox-core` owns gameplay and fixed-step time. `zombiebox-render3d` presents the world through `wgpu`.

The Android path is the current native target. iOS remains a future host: its view adapter, presentation path, and device build have not been implemented. If placing doors, walls, and occluders becomes the bottleneck, run the same street in Godot's tilemap editor and compare the time from layout edit to playable build. The headless rules and replay fixtures give both experiments a common reference. [Godot tilemaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html)

## Runtime ownership

| Owner | Responsibility | Across the boundary |
| --- | --- | --- |
| React Native shell | HUD, loading, pause, settings, hotbar, and overlay controls | Display status from the native host; send pause, settings, tool, and action input. |
| Android host | Run lifetime, focus, touch sampling, frame callbacks, audio/haptics playback, GPU surface lifecycle | Serialize run and surface operations on the rendering worker; drive `AudioTrack` SFX and haptics from simulation events. |
| Rust action runtime | 3D player and zombie poses, continuous collision, aiming, weapons, destructible structures, AI, and procedural world chunks | Advance the simulation at 60 ticks per second and expose poses, events, and HUD state to Rust rendering and JNI. |
| Rust 3D renderer | Camera rigs, chunk meshes, multi-level cutaways, lighting, actors, and transient effects | Present to an Android GPU surface or produce an offscreen RGBA frame for the browser host. |
| Browser action host | County and Birch Street action runs | Accept controls and return binary framebuffer and HUD data over HTTP. |
| Legacy browser host | Existing 20 Hz tile-command encounter and Canvas client | Keep existing browser inputs, HTTP transport, and deterministic command rules. |

The C ABI exposes the legacy command game and the native action run. Android retains a window reference for the surface lifetime. The action ABI also supports simulation-only ticks and HUD/audio reads for the native smoke check. [Public C header](../crates/zombiebox-ffi/include/zombiebox.h) · [Mobile component spec](../apps/mobile/specs/ZombieboxSurfaceNativeComponent.ts)

## Browser playtest

The dev host serves the action client at `/` and keeps runs in server memory. The browser sends sampled inputs with frame requests over HTTP keep-alive. The host advances the same 60 Hz fixed-step action simulation used on Android, renders with the shared headless GPU device, and returns ZBF2 binary frames with pixels, HUD data, and sound events. The browser presents the image in Canvas. Android owns and steps its run locally.

The `/legacy` route retains the original tile-command encounter. It sends batches of up to three commands to a 20 Hz run and draws the returned tile window, actors, HUD, and events. The headless example and existing legacy C callers still exercise those rules.

## One frame on a phone

1. The app starts in landscape with **Enter county** as the default. The native view samples the two sticks independently and transforms their screen directions to world axes. The thumbstick areas consume their own touches. Contextual interact, hotbar switching, quick-heal, and stick-integrated firing are consumed across simulation ticks.
2. The Android frame timestamp feeds a monotonic accumulator in the Rust action FFI. It advances at **60 ticks per second**, caps elapsed time after long frames, and retains discrete actions until a simulation tick consumes them. Pausing clears pending input and resets the accumulator.
3. After each tick, the FFI reads events before the next step clears them and informs both the renderer and the Android audio/haptics engine. The core resolves combat, destruction, AI awareness, physics, and outcomes. The renderer draws muzzle flashes, blasts, debris, lighting, and hit reactions. The HUD status packs health, ammunition, equipped tools, medkits, nearby threats, interact prompts, and outcome for the React Native layer.
4. The renderer uploads static chunk meshes when the loaded window or structure state changes. Actors, effects, lighting, visibility, and cutaways update independently. It depth-tests the scene at an internal resolution and presents a nearest-neighbor upscale to the surface. React Native draws the HUD and menus above it. [3D architecture](render3d-architecture.md)

The default camera uses a 2:1 isometric projection. The camera frame also maps thumbstick directions into world coordinates. The overhead, orbit, chase, and first-person views use the same simulation and room visibility. [Visual and performance target](product-decisions.md#visual-and-performance-target)

## Scaling from one street to the county

Expanding from the single Birch Street residential block to a **Minecraft-style seed-driven procedural world** and **drivable vehicles** requires evolving five architectural boundaries cleanly:

- **Two-tier procedural world generation.** `World::new(seed)` first fixes the terrain raster, district bounds, connected roads, public approaches and lot frontages. Parcel template choices use stable district ordinals. Looking up a tile or requesting a 16 × 16 chunk materializes detail for the intersecting parcels and reuses it across seams. Separate hash domains control layout, outdoor details, loot and infected. `World::chunk_sites` returns intersecting buildings and the loot and infected owned by that chunk across all floors. The full atlas explicitly requests the complete manifest. [Geography](../crates/zombiebox-core/src/world/geography.rs) · [Parcel queries](../crates/zombiebox-core/src/world/parcels.rs)
- **Atlas presentation.** The browser host serves a compact row-major survey under `/api/county/<seed>`. The Android atlas Activity serves the same JSON from Rust on a WebView request thread and loads the bundled Canvas assets offline. The map reads the generated features for collision, stair transitions and cutaways. [Survey format](../crates/zombiebox-core/src/survey.rs) · [Android atlas](../apps/mobile/android/app/src/main/java/game/zombiebox/mobile/CountyActivity.kt)
- **County action geometry and streaming.** The action stage turns generated terrain and district templates into walkable, destructible geometry. It materializes chunks around the player, updates the active window across chunk boundaries, and retains visited state in memory for the run. Generated infected IDs follow spawn ownership. The renderer draws the local isometric view and multi-floor cutaways. Future high-speed travel will need streaming and frame pacing checks. [County action stage](../crates/zombiebox-core/src/action/county.rs)
- **Throttled, invalidatable pathfinding & distant hordes.** Keep pathfinding scoped locally across active chunks/floors with local invalidation when doors open or windows/walls break. Model distant road hordes abstractly on the macro road graph and materialize/reconcile them into individual monsters as they enter or leave the active simulation radius.
- **Vehicle physics & camera/renderer headroom.**
  - **Vehicle simulation:** Give vehicles continuous physics state in `zombiebox-core` (for motorcycles: position, velocity, heading, speed, handlebar steer angle, body lean angle, surface traction, rider link, and unmanned skid/tumble state) running at the same 60 Hz fixed tick so wipeouts, rider ejection, skid injuries, and collisions with destructible props or infected stay deterministic and testable headlessly.
  - **Vehicle camera:** The chase rig can follow the motorcycle's heading so handlebar steering and rider lean align with screen directions. High-speed play will need checks for sight distance, chunk streaming, and frame pacing. Audio and haptics can consume events independently of the fixed-step simulation.

The GPU renderer lives in `crates/zombiebox-render3d` and depends on the simulation core. The Android host is in `apps/mobile/android`.

## Platform and persistence

- **iOS (future).** Reuse the Rust gameplay core and renderer with an iOS host and native presentation path. No iOS adapter or device build is included yet. [CAMetalLayer](https://developer.apple.com/documentation/QuartzCore/cametallayer) · [Packaging an XCFramework](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle)
- **Android.** Build `zombiebox-ffi` for the configured ABIs and link the static Rust library plus JNI C adapter into the app's shared library. The native worker presents frames to a `SurfaceView`. The local sideload APK uses the machine's Android debug signing key. [APK and device verification](../apps/mobile/README.md#verification) · [JNI tips](https://developer.android.com/training/articles/perf-jni)
- **GPU.** Android uses `wgpu` with its available surface adapter. The browser host uses a shared headless adapter. A future iOS host can reuse the renderer through a Metal surface. [wgpu backends](https://wgpu.rs/)
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
