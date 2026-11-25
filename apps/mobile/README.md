# Android app

The Android build runs the action game locally. The game surface is a native Android view that calls the Rust action runtime and software renderer through JNI. Rust renders RGBA pixels into an Android bitmap. React Native draws the HUD, menus, and action controls above the game surface. The repository also includes the browser playtest with its tile-command rules.

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

Gradle builds the Rust native library for the configured Android ABIs and packages the React Native bundle. The release variant uses the machine's local Android debug signing key so the APK can be sideloaded. The distributable workspace copy is `apps/mobile/build/zombiebox-android.apk`.

With USB debugging enabled and the phone listed by `adb devices`, install or update the app with:

```sh
adb install -r apps/mobile/build/zombiebox-android.apk
```

You can also transfer `apps/mobile/build/zombiebox-android.apk` to the phone, open it in the Downloads app or a file manager, and tap it to install. Android may ask you to allow that file manager to install unknown apps; enable that permission for the install, then confirm **Install**. The app opens in landscape and runs without a server. Tap **Enter the block** on the opening screen. The local debug key is for development and sideloading; it is not a Play Store signing key.

## Verification

- **Release APK.** The signed sideload build is at `apps/mobile/build/zombiebox-android.apk`. Rust tests, formatting, Clippy, the C smoke test, and mobile type checking and Codegen passed.
- **Host checks.** A seed `42` run cleared the block with five health and two rounds left after one wall-blocked shot. It covered one-time cache pickup, simultaneous movement and aim, and retained aim. A C ABI check verified that resetting the clock discards a queued action and resumes without catching up elapsed ticks. At 800 × 360 on the build host, p95 render time was 1.25 ms for software rendering alone.
- **Current APK on emulator.** A clean install launched in landscape in airplane mode on an API 35 x86_64 emulator. Two runs reached **Block cleared**. The playtest verified HUD updates for health, ammo, and infected count, simultaneous movement and aiming, one-time cache pickup, an aim-stick tap that preserved ammo, retained aim while moving, and a wall-blocked shot that spent one round. The scene stayed fixed while paused. Backgrounding an active run with both sticks held cleared the input on return, and play continued without a visible catch-up burst. **Restart run** and **Play again** each started a fresh run.
- **Physical handset.** Birch Street playthrough and frame pacing on a modest Android handset are pending. [Handset completion checks](../../docs/product-decisions.md#planned-milestones-and-completion-checks)

## Controls and encounter

- Drag the **left stick** to move. Movement is analog and slides along house walls.
- Drag the **right stick** to aim. The right stick only aims; its last direction stays active after you release it.
- Tap **Search**, **Fire**, or **Use**, or tap the scene outside both thumbsticks and the overlay controls. Near the pistol cache the action searches. After pickup, a tap fires one round in the current aim direction.
- Walk through the lit doorway and approach the warm-lit pistol cache inside the furnished house. Search once for a pistol and six rounds. Aim around walls and clear the three zombies to win.
- The HUD shows health, weapon and ammunition, the current objective, and remaining infected. Pause suspends the native simulation.

## What runs where

The Android native host owns the run handle, frame callbacks, bitmap surface, touch sampling, and JNI calls. Rust advances the fixed-step clock and owns player and zombie positions, collision, aim, shots, ammo, health, and win/death state. JNI locks the Android `Bitmap` while the Rust software renderer draws the isometric block and sprites into its RGBA pixels. React Native renders the HUD and controls over that native surface.

The game uses seeded residential world geometry and the house, door, and cache location from `zombiebox-core`. The action runtime is separate from the legacy 20 Hz tile-command `Game`; the browser playtest continues to use its existing controls and rules.

## Fabric surface contract

`specs/ZombieboxSurfaceNativeComponent.ts` defines the `ZombieboxSurface` Fabric component contract for the native view. It identifies a run with a decimal-string `seed` and `runId`, accepts a `paused` flag, and reports HUD changes. A string preserves an unsigned 64-bit seed across the JavaScript boundary. The Android implementation and its touch controls are in `android/`; there is no iOS host in this app yet.

The standalone Codegen check can be run from this directory:

```sh
npm ci
npm run codegen
```

React Native is pinned to 0.88.0. The project's Android SDK values are platform 37.0.0, Build Tools 37.0.0, and NDK 27.1.12297006; the Rust build targets `aarch64-linux-android` and `x86_64-linux-android`. The Rust JNI build script needs a Linux or macOS host. [React Native versions](https://reactnative.dev/versions) · [Codegen](https://reactnative.dev/docs/0.88/the-new-architecture/using-codegen) · [Android SDK tools](https://developer.android.com/tools)

## iOS

iOS is a future port. The Rust core and renderer are platform-independent, but this repository does not yet include an iOS app, native view adapter, Metal presentation path, or an iOS device build. A future host can reuse the gameplay rules and adapt the framebuffer to a UIKit/Metal surface while providing equivalent movement, aim, and action controls.
