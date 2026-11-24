# Zombiebox architecture

## Stack decision

Use React Native and Rust for the first playable experiment. A health meter or backpack is ordinary app UI. A street full of moving zombies, smoke, and roof cutaways benefits from a single renderer with direct access to the simulation. Mount one Fabric component for the world and layer React Native controls over it. Keep touch tracking, the game clock, and sprite positions inside the native game view and its Rust runtime. Fabric supports this kind of custom platform view, and React Native documents the separate timing constraints of the JS and UI threads. [Fabric native components](https://reactnative.dev/docs/fabric-native-components-introduction) · [React Native performance](https://reactnative.dev/docs/performance)

The cost is the pair of platform adapters and the tools needed to author buildings and inspect collisions. Prove the adapters with one street on both phones before producing a county of assets. If placing doors, walls, and occluders becomes the bottleneck, run the same street in Godot's tilemap editor and compare the time from layout edit to playable build. The headless rules and replay fixtures give both experiments a common reference. [Godot tilemaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html)

## Runtime ownership

| Owner | Responsibility | Across the boundary |
| --- | --- | --- |
| React Native shell | Run selection, inventory, pause UI, accessible controls | Mount the surface with a run ID, seed, and pause state. Receive HUD changes. |
| Native host on each OS | Run lifetime, focus, touch capture, display callbacks, GPU surface ownership | Queue input and lifecycle changes on one game-owner thread. Attach a replacement surface to the existing run. |
| Rust simulation | The map, searched caches, survivor, zombies, damage, death | Consume one command per tick. Expose a borrowed snapshot, a small HUD value, and tick-local gameplay events. |
| Rust renderer (planned) | Camera, tile and sprite batches, occlusion, explosions | Read visible world state and consume events inside Rust. Retain only presentation data between frames. |

The checked-in C ABI lets a native owner create a run, step it, copy its HUD, and release it. Handles stay on the native side. The renderer will use the core crate directly. Add GPU surface entry points at the first device spike, when the actual platform lifetimes can be exercised. [Public C header](../crates/zombiebox-ffi/include/zombiebox.h) · [Mobile component spec](../apps/mobile/specs/ZombieboxSurfaceNativeComponent.ts)

## Browser playtest

The dev host owns each `Game` in Rust and serves a Canvas client. The client drives the 20 Hz clock, sends ordered batches of up to three commands, and renders the returned tile window, actors, HUD, and tick events. The simulation advances only when a command batch arrives. This lets Android Chrome test the encounter and touch controls on the Debian host through Tailscale.

The browser transport and Canvas view are specific to the playtest. The native Android view will own its game locally and can reuse the core's commands, snapshots, and events. The iOS adapter can use the same core contracts and native renderer after the Android device experiment.

## One frame on a phone

1. Collect thumb movement and action edges in the native view. Inverse-project screen direction into world axes so dragging right always feels like moving right on screen. Consume an attack tap once, including when several ticks fall into a frame.
2. Accumulate foreground time on a monotonic clock. Advance Rust at **20 ticks per second**, inserting a wait when idle. Start with a three-tick catch-up limit per display callback and discard excess backlog. On pause, clear held input and reset the clock. Resume from the same simulation tick.
3. After each tick, read the emitted events before the next step clears them. Store the previous and current actor poses for interpolation. An explosion applies its damage in the core. The renderer can spend its own effects budget on flash, smoke, and debris. Hearing and horde attraction will also use the simulation event.
4. Draw visible tiles and sprites on the display cadence. Deliver a HUD event when health, supplies, or equipment changes. The native host sends the initial HUD again when a view attaches.

For the low-fi look, start with a small nearest-filtered atlas and a 2:1 diamond camera. For tile coordinates `(x, y)`, the ground projection is `((x − y) × tileWidth/2, (x + y) × tileHeight/2)`. Draw floors first, then order standing sprites and walls by projected foot position with a stable tie-break. Fade roofs around the player. Batch debris sprites, and keep enemy silhouettes legible during the flash.

## Scaling from one street to the county

The core can already regenerate any 16 × 16 chunk by coordinate. For the phone renderer, cache the visible chunks plus a margin and evict distant tile batches. Track active zombies in spatial buckets so a blast or noise only queries nearby cells. Give moving entities stable IDs independent of their current chunk. A zombie crossing a road seam changes its spatial bucket and keeps its identity.

Keep the first renderer in a sibling crate, `crates/zombiebox-render`, depending on the core. Put the device view adapters in `apps/mobile/ios` and `apps/mobile/android`. Add these at the device milestone. A car can later use the same world coordinates and spatial queries, with a larger collision footprint and a longer loading distance in its direction of travel.

## Platform and persistence

- **iOS.** Back the Fabric view with a Metal layer. Build Rust as a static library and package separate device and simulator slices in an XCFramework. The layer may disappear during a running session. Release its rendering resources while the native session continues owning the game. [CAMetalLayer](https://developer.apple.com/documentation/quartzcore/cametallayer) · [Packaging an XCFramework](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle)
- **Android.** Pass the view's native window through a thin JNI adapter. Package the Rust shared library for each tested ABI with Gradle and the NDK. Stop use of the window before the surface destruction callback returns. Verify both touch routing and RN overlay composition on the chosen surface. [Native windows](https://developer.android.com/ndk/reference/group/a-native-window) · [Surface lifecycle](https://developer.android.com/reference/android/view/SurfaceHolder.Callback.html) · [cargo-ndk](https://github.com/bbqsrc/cargo-ndk)
- **GPU.** Prototype wgpu with Metal on iOS and Vulkan on Android. Confirm adapter availability on the oldest devices selected for testing. Keep platform differences behind surface creation and presentation. [wgpu backends](https://wgpu.rs/)
- **Saves (planned).** At a tick boundary capture the seed, generator and save-format versions, player inventory and position, consumed loot, entity state, and chunk edits. Checkpoint after meaningful changes and when backgrounded. Write a temporary save beside the prior checkpoint and atomically replace it after a successful write. Rebuild baseline chunks with their recorded generator version. A death checkpoint marks the run ended.

## First device decision

The headless example exercises the game rules, the C smoke test exercises native linking, and the browser playtest exercises touch controls. The native milestone is a **single residential block on a midrange Android phone**, followed by the same experiment on an iPhone. Log device models, OS versions, and release build settings alongside the measurements.

| Exercise | Initial acceptance target |
| --- | --- |
| Walk, scavenge, and fire under a React Native HUD | One tap spends one charge. The blast appears at the simulation position and the HUD agrees with the core. Opening inventory pauses the run. |
| Lock and unlock, resize, and remount the surface repeatedly | Position and loot survive. The clock resumes without a catch-up burst. A destroyed surface receives no further frames. |
| Populate a street with 100 visible moving zombies and trigger three overlapping blasts | Target 60 Hz. Record p95 frame time against 16.7 ms, tick CPU cost, memory, and sustained behavior over ten minutes. These numbers are budgets to test. |

Once the device slice passes, build the first return trip with a saved inventory and a depleted pharmacy. Expand district encounters and distant hordes after that loop feels good. Vehicle streaming gets its own cross-county test. [World design and milestones](world-design.md)
