# Mobile host

This directory defines the React Native side of the first device experiment. One Fabric view presents the game, while ordinary React Native views display inventory, menus, and the HUD. The check below generates bindings for the game surface. A native host on each platform is the next milestone.

## Generate the bindings

Use Node 22.13+ within the 22.x series, or another supported version listed in `package.json`. The manifest pins React Native and its Codegen tool to 0.88.0. Run from this directory. [React Native versions](https://reactnative.dev/versions) · [Using Codegen](https://reactnative.dev/docs/0.88/the-new-architecture/using-codegen)

```sh
npm ci
npm run codegen
```

The standalone check installs only Codegen and its dependencies. It writes Android bindings under `build/android/` and iOS bindings under `build/ios/`. The future host projects will install the pinned React and React Native peers and run Codegen from Gradle and Xcode as part of their builds.

## Surface contract

`specs/ZombieboxSurfaceNativeComponent.ts` names the native component `ZombieboxSurface`. The shell can mount it as follows once both native view adapters exist.

```tsx
import GameSurface from './specs/ZombieboxSurfaceNativeComponent';

<GameSurface
  style={{flex: 1}}
  runId={run.id}
  seed={run.seed}
  paused={inventoryOpen}
  onHudChanged={({nativeEvent}) => setHud(nativeEvent)}
/>
```

- `runId` identifies one run across surface mounts. The host attaches the named session on mount and detaches on unmount. A fresh run gets a new ID, even when replaying a seed.
- `seed` is a decimal string representing an unsigned 64-bit integer, immutable for the lifetime of a run. A string preserves every Rust seed across JavaScript's numeric boundary.
- `paused` lets the shell suspend the simulation while a menu is open. The host also suspends play when the app becomes inactive or loses the drawing surface. On resume, reset the time accumulator and clear held touches.
- `onHudChanged` sends health, supply count and equipped-weapon status on attach and when those values change. Publish one complete HUD snapshot after a tick. Movement, aiming and world drawing stay in the native view and Rust runtime.

## Android bring-up on Debian

The repository has the Fabric spec, Rust library, and a C smoke test. The Gradle app, JNI adapter, and native renderer are still to be built. Use the [browser playtest](../../README.md#play-on-android) for phone play while those pieces are in development.

Install JDK 17, a Node version supported by `package.json`, and the Android SDK command-line tools. The React Native 0.88.0 app template uses Android platform 37, Build Tools 37.0.0, and NDK 27.1.12297006. Set `ANDROID_HOME` to the SDK path and install these packages from `sdkmanager` along with `platform-tools` for `adb`. [Android SDK command-line tools](https://developer.android.com/tools) · [React Native 0.88.0 Android template](https://github.com/react-native-community/template/blob/0.88.0/template/android/build.gradle)

```sh
sdkmanager 'platform-tools' 'platforms;android-37' 'build-tools;37.0.0' 'ndk;27.1.12297006'
rustup target add x86_64-linux-android aarch64-linux-android
```

For an emulator, install the `emulator` package and an x86_64 Android system image at API 24 or newer, create an AVD, and verify KVM access using `emulator -accel-check`. The Rust build for that emulator uses `x86_64-linux-android`. For an arm64 phone, use `aarch64-linux-android`, enable USB debugging, and check that `adb devices` shows it as `device`. [Android Emulator acceleration](https://developer.android.com/studio/run/emulator-acceleration) · [Android Debug Bridge](https://developer.android.com/tools/adb) · [Android Rust targets](https://doc.rust-lang.org/rustc/platform-support/android.html)

### First installable milestone

1. Add the pinned React Native Gradle host in `apps/mobile/android`. Package `zombiebox-ffi` as a shared library for the selected ABI and load it from a thin JNI adapter. Keep the run handle and its tick clock on one owner thread. Register the generated `ZombieboxSurface` view and publish its initial HUD through the Fabric event.
2. Use a simple native view to mark the player's position while exercising movement and the existing `zb_v1_*` entry points. Run the seed-42 sequence `Scavenge`, `Attack`, `West`, `Attack` with ticks controlled by the smoke test. The HUD should end at tick 4, one tile west of the spawn with full health and one charge left. Use the headless example to check zombie removal until the renderer has access to world snapshots. [Headless example](../../crates/zombiebox-core/examples/survival.rs) · [C interface](../../crates/zombiebox-ffi/include/zombiebox.h)
3. Install the debug APK and exercise focus loss, inventory pause, and view detach and reattach. Verify that the paused tick stays fixed, a remount with the same run ID restores the HUD, and closing the run releases its handle after callbacks stop. Repeat on an arm64 handset once the emulator smoke test passes.

## Native rendering and iOS reuse

**Android.** Register a Fabric view manager named `ZombieboxSurface` around a `SurfaceView`. Obtain the `ANativeWindow` from the live Java surface through JNI. Send touch input to the session and pass resize events to the renderer. Stop presentation and wait for the render thread to release the surface before returning from `surfaceDestroyed`. Package the Rust shared library for each tested ABI with the Android NDK through Gradle. [Fabric native components](https://reactnative.dev/docs/0.88/fabric-native-components-introduction) · [Surface callbacks](https://developer.android.com/reference/android/view/SurfaceHolder.Callback.html) · [cargo-ndk](https://github.com/bbqsrc/cargo-ndk)

**iOS.** Reuse the Fabric spec, Rust run, C header, and seed-42 sequence on macOS with Xcode. Implement `RCTZombieboxSurface` as a Fabric component view containing a Metal-backed `UIView`. A display link requests frames while the scene is active. Pass touch events and drawable size changes to the runtime, and stop the display link before detaching the Metal layer. Link Rust static libraries built for the iOS device and simulator SDKs via an XCFramework. [Metal layers](https://developer.apple.com/documentation/QuartzCore/CAMetalLayer) · [Rust iOS targets](https://doc.rust-lang.org/rustc/platform-support/apple-ios.html)

**Device acceptance.** On an iPhone and an Android handset, move a sprite under a React Native HUD, open and close inventory to pause and resume, background the app, then detach and reattach the view. Confirm the same survivor returns, input releases correctly, and frame pacing remains steady during a particle burst.
