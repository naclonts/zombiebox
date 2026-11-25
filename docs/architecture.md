# Zombiebox architecture

## Stack decision

The first local Android app uses React Native for the HUD, menus, and action controls, with a native landscape view for the game. The Android host calls a separate float-position action runtime in `zombiebox-core` and the sibling software renderer through JNI. Rust renders into the pixels of the Android bitmap. The Rust simulation owns gameplay and fixed-step timing, while the Android view owns touch sampling and presentation. [React Native performance](https://reactnative.dev/docs/performance)

The Android path is the current native target. iOS remains a future host: its view adapter, presentation path, and device build have not been implemented. If placing doors, walls, and occluders becomes the bottleneck, run the same street in Godot's tilemap editor and compare the time from layout edit to playable build. The headless rules and replay fixtures give both experiments a common reference. [Godot tilemaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html)

## Runtime ownership

| Owner | Responsibility | Across the boundary |
| --- | --- | --- |
| React Native shell | HUD, loading and pause states, action control | Display status from the native host; send pause and action input. |
| Android host | Run lifetime, focus, touch sampling, frame callbacks, bitmap presentation | Own the run handle and call JNI with each frame and sampled controls. |
| Rust action runtime | Float player and zombie poses, collision, aiming, shots, health, ammo, outcome | Advance the residential encounter at 60 ticks per second and expose poses, events, and HUD state to Rust rendering and JNI. |
| Rust software renderer | Isometric world, house, cache, actors, and transient effects | Render current action state into a packed RGBA framebuffer for Android bitmap presentation. |
| Legacy browser host | Existing 20 Hz tile-command encounter and Canvas client | Keep existing browser inputs, HTTP transport, and deterministic command rules. |

The C ABI exposes both the legacy command game and a native action run that advances from frame timestamps, accepts normalized move and aim sticks, and renders into caller-owned pixel memory. Handles stay on the native side. Android passes a bitmap to the JNI adapter; the Rust renderer writes the RGBA framebuffer into it. [Public C header](../crates/zombiebox-ffi/include/zombiebox.h) · [Mobile component spec](../apps/mobile/specs/ZombieboxSurfaceNativeComponent.ts)

## Browser playtest

The dev host owns each `Game` in Rust and serves a Canvas client. The client drives the 20 Hz clock, sends ordered batches of up to three commands, and renders the returned tile window, actors, HUD, and tick events. The simulation advances only when a command batch arrives. This lets Android Chrome test the encounter and touch controls on the Debian host through Tailscale.

The browser host starts the residential scenario in the legacy tile-command runtime. Rust defines the playable bounds, house walls, doorway, and pistol cache. The headless example and existing C callers continue to start the original encounter. The browser uses its existing WASD/arrow, scavenge, fire, and pause controls.

The browser transport, Canvas view, and legacy rules are independent of the local Android action game. Android creates its own run from a seed and steps it locally. Both action runtime and legacy runtime use the seeded world and residential house geometry, while retaining separate gameplay inputs and update rates.

## One frame on a phone

1. The app starts in landscape and opens the block after **Enter the block**. The native view samples left-stick movement and right-stick aim independently and transforms screen directions to world axes. The thumbstick areas consume their own touches. A Search / Fire / Use button tap or a scene tap outside both thumbsticks and React Native overlays is consumed once, including when multiple simulation ticks occur during one display frame. Near loot it searches. After pickup it fires along the retained aim.
2. The Android frame timestamp feeds a monotonic accumulator in the Rust action FFI. It advances at **60 ticks per second**, caps elapsed time after long frames, and retains action taps until a simulation tick consumes them. Pausing clears pending input and resets the accumulator.
3. After each tick, the FFI reads events before the next step clears them and informs the renderer. The core resolves damage and outcomes. The renderer shows muzzle flashes, impact effects, and infected hit reactions and falls. The HUD status packs health, ammunition, zombie count, weapon, loot proximity, and outcome for the React Native layer.
4. The software renderer draws the landscaped street, furnished cutaway house, lit doorway and cache, and eight-direction survivor and infected sprites into a packed RGBA framebuffer. Movement drives walk and shuffle animations, and firing adds recoil. The native view presents the Android bitmap, with React Native drawing the HUD and action controls on top. [Street rendering](../crates/zombiebox-render/src/street.rs) · [Actor rendering](../crates/zombiebox-render/src/actors.rs)

The 2:1 diamond camera projects tile coordinates `(x, y)` to `((x − y) × tileWidth/2, (x + y) × tileHeight/2)`. Use projected foot positions for depth ordering and keep the player, routes, and enemies legible when walls, roofs, or effects obscure the scene. Choose assets, lighting, and rendering techniques that meet the visual target within the frame budget on modest Android hardware. [Visual and performance target](product-decisions.md#visual-and-performance-target)

## Scaling from one street to the county

The core can already regenerate any 16 × 16 chunk by coordinate. For a larger phone world, cache visible chunks plus a margin and evict distant tile batches. Track active monsters in spatial buckets so shots, blasts, sightlines, and noise query nearby cells. Give moving entities stable IDs independent of their current chunk. A monster crossing a road seam changes its spatial bucket and keeps its identity.

As the residential block expands to multiple multi-level houses, breakable tiles/props, and larger reactive monster groups, keep three architectural boundaries clean:
- **Data-driven building and material definitions.** Move beyond hardcoded house/prop coordinates in Rust toward reusable floor blueprints and a material durability table so multi-level layouts and destruction stay fast to author and persist as chunk edits.
- **Throttled, invalidatable pathfinding.** Replace per-tick full BFS with cached/throttled pathfinding across floors that invalidates locally when doors open or windows and walls break.
- **Renderer and audio headroom.** Keep an eye on CPU software-rendering cost as lights, multi-level cutaways, debris, and enemy counts grow (using physical Android handset pacing to decide when to adopt GPU rendering via `wgpu`), and keep audio playback cleanly decoupled from fixed simulation ticks so sound effects and haptics fire from simulation events without stalling frames.

The software renderer lives in `crates/zombiebox-render` and depends on the core. The Android host is in `apps/mobile/android`. An iOS adapter is still future work. A car can later use the same world coordinates and spatial queries, with a larger collision footprint and a longer loading distance in its direction of travel.

## Platform and persistence

- **iOS (future).** Reuse the Rust gameplay core and renderer with an iOS host and native presentation path. No iOS adapter or device build is included yet. [CAMetalLayer](https://developer.apple.com/documentation/QuartzCore/cametallayer) · [Packaging an XCFramework](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle)
- **Android.** Build `zombiebox-ffi` for the configured ABIs and link the static Rust library plus JNI C adapter into the app's shared library. The native view presents the rendering in a bitmap. The local sideload APK uses the machine's Android debug signing key. [APK and device verification](../apps/mobile/README.md#verification) · [Android bitmap API](https://developer.android.com/reference/android/graphics/Bitmap) · [JNI tips](https://developer.android.com/training/articles/perf-jni)
- **GPU.** Prototype wgpu with Metal on iOS and Vulkan on Android. Confirm adapter availability on the oldest devices selected for testing. Keep platform differences behind surface creation and presentation. [wgpu backends](https://wgpu.rs/)
- **Saves (planned).** At a tick boundary capture the seed, generator and save-format versions, player inventory and position, consumed loot, entity state, and chunk edits. Checkpoint after meaningful changes and when backgrounded. Write a temporary save beside the prior checkpoint and atomically replace it after a successful write. Rebuild baseline chunks with their recorded generator version. A death checkpoint marks the run ended.

## First device decision

The headless example exercises legacy game rules, core action tests cover the native encounter, and the C smoke test checks native linking. Report APK build, emulator, and handset results separately so a successful build is not mistaken for device validation. The next platform port is iOS; it requires a host and on-device validation before it can be treated as supported.

| Exercise | Initial acceptance target |
| --- | --- |
| Move, pick up tools, aim, and shoot under the React Native HUD | Movement feels brisk and shooting integrates cleanly with twin-stick ergonomics without requiring mid-screen thumb travel. Pickup works near caches, shots respect intact walls, and ammo, health, and threat count match the Rust action runtime. Pause freezes simulation ticks. |
| Play Birch Street on a modest Android handset | Record the handset model and frame pacing during movement, aiming, and repeated shots. Check that touch controls stay responsive and that threats, the doorway, and loot stay easy to read during combat. |
| Pause and resume or background and foreground the Android app | Input releases cleanly and the game clock resumes without a catch-up burst. |
| Stress a street with 100 visible zombies and repeated shots | Target 60 Hz. Record p95 frame time against 16.7 ms, tick CPU cost, memory, and sustained behavior over ten minutes. These numbers are budgets to test. |

Next, refine action playability and audio feedback, validate on a physical Android handset, and build out a high-quality destructible residential block with multi-level houses, expanded tools, reactive AI, and monster variety. Once the residential block feels great, connect the persistent medicine run, expand into a full city and county with distant hordes, and test vehicle streaming. [World design and milestones](world-design.md)
