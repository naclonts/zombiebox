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

## First native hosts

**Android.** Register a Fabric view manager named `ZombieboxSurface` around a `SurfaceView`. Obtain the `ANativeWindow` from the live Java surface through JNI. Send touch input to the session and pass resize events to the renderer. Stop presentation and wait for the render thread to release the surface before returning from `surfaceDestroyed`. Package the Rust shared library for each tested ABI with the Android NDK through Gradle. [Fabric native components](https://reactnative.dev/docs/0.88/fabric-native-components-introduction) · [Surface callbacks](https://developer.android.com/reference/android/view/SurfaceHolder.Callback.html) · [cargo-ndk](https://github.com/bbqsrc/cargo-ndk)

**iOS.** Implement `RCTZombieboxSurface` as a Fabric component view containing a Metal-backed `UIView`. A display link requests frames while the scene is active. Pass touch events and drawable size changes to the runtime, and stop the display link before detaching the Metal layer. Link Rust static libraries built for the iOS device and simulator SDKs via an XCFramework. [Metal layers](https://developer.apple.com/documentation/QuartzCore/CAMetalLayer) · [Rust iOS targets](https://doc.rust-lang.org/rustc/platform-support/apple-ios.html)

**Device acceptance.** On an iPhone and an Android handset, move a sprite under a React Native HUD, open and close inventory to pause and resume, background the app, then detach and reattach the view. Confirm the same survivor returns, input releases correctly, and frame pacing remains steady during a particle burst.
