# Android app

The Android build runs the action game locally. A native rendering worker calls the Rust action runtime and 3D renderer through JNI. `wgpu` presents to the Android `SurfaceView`. React Native draws the HUD, menus, and action controls above it.

## Build and sideload

Use Node 22, JDK 17, CMake 3.22.1, the Android SDK command-line tools, and Rust. Set `ANDROID_HOME` to the SDK installation. Install Android platform 37, Build Tools 36.0.0 and 37.0.0, CMake, NDK, and platform tools, plus the Rust targets used by the Gradle project:

```sh
sdkmanager 'platform-tools' 'platforms;android-37.0' 'build-tools;36.0.0' 'build-tools;37.0.0' 'cmake;3.22.1' 'ndk;27.1.12297006'
rustup target add aarch64-linux-android x86_64-linux-android
```

Install the JavaScript dependencies and build the release APK. Run Gradle from `apps/mobile/android`:

```sh
cd apps/mobile
npm ci
cd android
./gradlew :app:assembleRelease
mkdir -p ../build
cp app/build/outputs/apk/release/app-release.apk ../build/zombiebox-android.apk
```

Gradle builds the Rust native library for the configured Android ABIs and packages the React Native bundle. The release variant uses the machine's local Android debug signing key so the APK can be sideloaded. The distributable workspace copy is `apps/mobile/build/zombiebox-android.apk`. From the repository root, `make build` (or `make apk`) runs the same steps and prints the APK SHA-256, and `make install` pushes it with `adb install -r`.

The settings menu reports the build stamp (`BUILD <short-commit>`, suffixed `-dirty` when the working tree has tracked changes). `scripts/write-git-info.cjs` writes the generated `gitInfo.ts` that App.tsx imports; Gradle's `generateGitInfo` task runs it before the release JavaScript bundle, and `npm ci` runs it through `postinstall` for development builds.

With USB debugging enabled and the phone listed by `adb devices`, install or update the app with:

```sh
adb install -r apps/mobile/build/zombiebox-android.apk
```

You can also transfer `apps/mobile/build/zombiebox-android.apk` to the phone, open it in the Downloads app or a file manager, and tap it to install. Android may ask you to allow that file manager to install unknown apps, then confirm **Install**. The app opens in landscape and runs offline. Tap **Enter county** to start the default scenario, or select **Birch Street** and tap **Enter the block**.

## Verification

- **Pre-migration county release APK and emulator.** The signed arm64-v8a and x86_64 release APK built successfully. On the API 35 x86_64 emulator in airplane mode, seed 42 opened in the residential district. Movement, pause and resume, direct deployment from the atlas, same-seed teleport to the hospital, return from the atlas, and Birch Street selection passed.
- **Pre-migration county rendering on the host.** Seed 42 captures at `800 × 360` covered county spawn, downtown, the hospital, the army base, woods, and Birch Street. In a release build on an Intel i7-10750H, the p95 for stationary simulation and rendering ranged from 1.46 to 4.10 ms across those scenes. Host tests exercise cross-chunk movement and retention of cache and structural state across atlas teleports.
- **Pre-migration 2.5D implementation (host-verified).** The expanded Birch Street residential block (3 two-story destructible houses, 6 streetlights, 8 caches, 12 reactive infected across 4 archetypes, Axe/Pistol/Shotgun/Rocket Launcher/Medkit, dusk lighting & room visibility, `zb_action_v2` FFI/JNI, 3 twin-stick control schemes, procedural Genesis/PS1 `AudioTrack` SFX + Android haptics, and updated Fabric/React Native HUD & Settings) passed `zombiebox-core` tests (36/36), `zombiebox-render` tests (2/2), strict C11 syntax checks on `host.c` & `zombiebox_jni.c`, and mobile `npm run codegen` & `npm run typecheck`. At `800 × 360` on the build host (release), software rendering measured `p95 = 1.64 ms` on the full residential block and `p95 = 1.69 ms` with 100 stress zombies; four rendered scenes (`street`, `interior`, `upstairs`, `combat`) were visually verified.
- **Pre-migration release APK & emulator.** Signed sideload APK rebuilt cleanly at `apps/mobile/build/zombiebox-android.apk` (`:app:assembleRelease` succeeded across `arm64-v8a` and `x86_64` Rust/JNI, Kotlin/Java, Fabric Codegen, Hermes JS bundle, and `lintVitalRelease`). Emulator playthrough of the expanded Birch Street block is pending. (Prior one-house APK verification passed offline on an API 35 x86_64 emulator.)
- **County atlas emulator (2026-11-25).** The release APK (SHA-256 `93ccec00fcbf603276ea6743c784cd0a60d6d67ff2119259306ad0463bfe6c37`) was installed on an API 35 x86_64 emulator. With airplane mode enabled, the atlas opened from the menu, generated seed 42, started a walk and moved east, then generated seed 312 offline; Android Back returned to the menu. This was an atlas smoke test, not a Birch Street playthrough.
- **3D release APK and emulator smoke.** Offline `assembleRelease` produced `apps/mobile/build/zombiebox-android.apk` (SHA-256 `c7e3c7362fb681e5f23b0f10ef20b0f6bdbee860479bf4256aac9370cbd511f9`) with `arm64-v8a` and `x86_64` JNI libraries. On an offline API 35 x86_64 emulator using SwiftShader at 2400×1080, seed 42 movement changed tile `(354, 163)` to `(352, 163)`. Aim input, the TOP camera, pause, Home, foreground and resume passed with the same HUD location. The offline atlas teleported the run to `(324, 175)` and returned to gameplay. A 118-sample run logged p50 100.98 ms, p95 153.52 ms and p99 172.24 ms at 67% resolution with concurrent host work; the next window reported p95 139.81 ms. This does not measure handset performance.
- **Motorcycle Android verification (2026-11-26).** The final offline release APK built successfully for `arm64-v8a` and `x86_64`, passed native host checks, and was installed on the API 35 x86_64 emulator. Final APK: `apps/mobile/build/zombiebox-android.apk`, SHA-256 `d991d17241c47e0aadcc7f858fc942230fbcd2f73393559023b78ebe05ee1080`. The final build launched seed 42, entered county, deployed to the starter bike at `(354, 166)`, and mounted successfully. The bike and rider rendered; mounted HUD showed BIKE 100%, 0 MPH, and DISMOUNT. Settings switched between CHASE and ISOMETRIC while mounted; screenshots confirmed the distinct behind-the-bike and fixed elevated views. Pause, Home/background, foreground and resume preserved the paused mounted session, and Resume returned to BIKE 100% at `(354, 166)`. On the immediately previous build, throttle plus steering and lean moved from `(354, 165)` to `(354, 162)` and reached 3 MPH, braking returned to 0 MPH, and dismount returned to foot play. Active held-touch clearing across backgrounding was not tested. The emulator's SwiftShader pacing was poor (p95 about 132–156 ms); this is not handset performance data. A full route playthrough and physical Samsung controls/pacing coverage remain outstanding.
- **Physical handset.** The user confirmed that the 3D build works on a physical Android handset. The device model, exact playthrough coverage, and frame timing were not supplied. A full Birch Street clear, detailed controls and touch/audio/haptic latency checks, thermal behavior, and sustained frame pacing remain pending. [Handset completion checks](../../docs/product-decisions.md#planned-milestones-and-completion-checks)

- **Build stamp and Makefile (2026-11-26).** `make build` assembled the release APK offline-incrementally (`BUILD SUCCESSFUL`, 3m 6s on this host) and wrote `apps/mobile/build/zombiebox-android.apk` (SHA-256 `5084e503f2a333666697f53e2372bc301ea63db28986cae887a4c5940ce0d97e`). The Hermes bundle inside that APK contains the `BUILD e8b4db0-dirty` stamp, and `npm run typecheck` passes with the generated `gitInfo.ts`. `make run` served `http://127.0.0.1:8765/` (HTTP 200). The settings-menu placement of the stamp still needs a visual check on a device or emulator.

For 3D verification, see the [progress ledger](../../docs/render3d-progress.md#3d-cutover-verification). In an Android development session, `adb logcat -s Zombiebox3D` can show frame interval percentiles, CPU preparation and submission times, culling, and the active resolution tier.

### County field atlas

Choose **County atlas** on the opening menu or **Map** during play. Enter a decimal seed or survey reference seed **42**. Select a landmark to inspect its site and floors, or use **Start walk** and the directional pad to explore. The plus and minus buttons change floors when standing on stairs. Choose **Play at this cell** to start at the selected spot. **Teleport here** moves the survivor within an active same-seed run and keeps their gear and world edits. Use **Back to game** or Android Back to leave the atlas at the current position. The APK bundles the atlas assets and generates each county locally.

## Controls and encounter

Settings offers five camera views. Isometric is the default.

- Drag the **left stick** to move briskly; light tilt (`<= 52%`) walks quietly without emitting footstep noise.
- Use the **right stick** to aim and fire without moving your thumb to the center of the screen. Select your preferred scheme in **Settings**:
  - **Tap Stick (default):** drag to aim (aim stays locked on release), tap the right stick to fire/swing, or double-tap & hold for continuous fire/chopping.
  - **Outer Ring:** inner 78% aims; push past the amber ring to auto-fire.
  - **Release:** drag past deadzone to aim and release to fire; pull back to center to cancel safely.
  - In **Chase** and **First** camera views the right stick looks around (light tilt aims precisely, full deflection turns quickly); the left stick moves relative to the view and fire follows the camera. **Iso**, **Top**, and **Orbit** keep the screen-relative twin-stick mapping.
- Tap the **bottom hotbar** (`AXE`, `PST`, `SHT`, `RKT`) to switch tools (tap the active firearm or **RLD** to reload), tap **+ MED** to quick-heal without changing weapons, and tap the contextual **Interact** button (or tap the scene while an interact prompt is shown) to search caches, open/close doors, vault broken windows, drop from 2F windows, or take stairs.
- In the county, follow roads between districts, enter buildings, and scavenge for supplies. The HUD shows the current district and tile, floor, and nearby infected count.
- **Motorcycles:** mount with **Interact** near an operative bike, including while it is rolling or skidding. While riding, the left stick steers and throttles/brakes; hold it down at a stop to push backward at walking pace, and use the right stick to lean. Press **Interact** to dismount at rest or while moving. A moving dismount leaves the bike coasting, so you can chase and remount it while it rolls.
- Explore Birch Street's three multi-level houses and yards: grab the **Axe** on the front stoop (shatters glass in 1 hit), **Pistol** inside Birch Bungalow, **Shotgun** in Elm Corner Duplex, **Rocket Launcher** upstairs in Oak Ridge Split-Level (breaches structural walls & collapses unsupported 2F overhangs), and **Medkits** / ammo caches. Clear all 12 sight- and sound-reactive threats (**Shamblers**, **Brisk** runners, **Bloaters**, and telegraphed **Shooters**).
- **Settings** (top-right gear or Start/Pause menu) controls twin-stick scheme, SFX volume & mute, Android haptics, and camera aim look-ahead. Pause suspends the native simulation and clears held input; backgrounding suspends audio cleanly.

## What runs where

The Android host samples touch and forwards input to a worker that owns the run handle and the GPU surface. Rust advances the fixed-step clock and owns player and infected positions, collision, aim, ammo, health, and outcomes. The worker releases its window reference when the surface is destroyed and clears held input when play pauses.

The county action run streams nearby terrain, buildings, caches, and infected from `zombiebox-core`. Android and the default browser playtest share the action simulation and renderer. The legacy browser route uses the original 20 Hz tile-command `Game`.

## Fabric surface contract

`specs/ZombieboxSurfaceNativeComponent.ts` defines the `ZombieboxSurface` Fabric component contract for the native view. It identifies a run with its `scenario`, decimal-string `seed`, and `runId`. It accepts a `paused` flag and reports HUD changes and atlas teleports. A string preserves an unsigned 64-bit seed across the JavaScript boundary.

The standalone Codegen check can be run from this directory:

```sh
npm ci
npm run codegen
```

React Native is pinned to 0.88.0. The project's Android SDK values are platform 37.0.0, Build Tools 37.0.0, and NDK 27.1.12297006; the Rust build targets `aarch64-linux-android` and `x86_64-linux-android`. The Rust JNI build script needs a Linux or macOS host. [React Native versions](https://reactnative.dev/versions) · [Codegen](https://reactnative.dev/docs/0.88/the-new-architecture/using-codegen) · [Android SDK tools](https://developer.android.com/tools)

## iOS

iOS is a future port. The Rust core and renderer are platform-independent, but this repository does not yet include an iOS app, native view adapter, Metal presentation path, or an iOS device build. A future host can reuse the gameplay rules and attach the renderer to a UIKit/Metal surface while providing equivalent movement, aim, and action controls.
