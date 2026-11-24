# Zombiebox

A single-player isometric zombie survival sandbox. The local Android app is a landscape twin-stick action game: enter the house on Birch Street, pick up a pistol, and clear three zombies from the block. The browser playtest remains available, and an iOS app is a future platform target.

## Run the browser playtest

From the repository root, start the dev server with stable Rust 1.87 or newer.

```sh
cargo run --locked -p zombiebox-web
```

Open `http://127.0.0.1:8765`. Follow the lit doorway and stand on the marked crate to scavenge. **Fire** highlights when the pistol has a clear shot. Use the on-screen controls or move with **WASD** or the **arrow keys**, scavenge with **L**, fire with **Space**, and pause with **Escape**.

### Install the Android app

Build a sideloadable release APK from the repository root. This requires Node 22, JDK 17, CMake 3.22.1, Android SDK platform 37, Build Tools 36.0.0 and 37.0.0, NDK 27.1.12297006, and the Rust Android targets. The Gradle release variant uses the local Android debug signing key for sideloading.

```sh
cd apps/mobile
npm ci
rustup target add aarch64-linux-android x86_64-linux-android
cd android
./gradlew :app:assembleRelease
mkdir -p ../build
cp app/build/outputs/apk/release/app-release.apk ../build/zombiebox-android.apk
```

The APK is written to `apps/mobile/build/zombiebox-android.apk`. To install it on a USB-connected Android phone with USB debugging enabled:

```sh
adb install -r apps/mobile/build/zombiebox-android.apk
```

The app runs locally on the phone and does not need the browser server. It opens in landscape. Drag the left stick to move and the right stick to aim; aiming does not fire. Tap the **Search**, **Fire**, or **Use** action button, or tap the game scene. Near the pistol cache the action searches; after pickup it fires in the current aim direction. Move through the house doorway and approach the pistol to collect it. A clear shot is required to hit a zombie.

To install manually, transfer the APK to the phone and open it from Downloads or a file manager. If prompted, allow that app to install unknown apps, then confirm installation. See the [mobile guide](apps/mobile/README.md) for SDK setup and current emulator verification.

The APK build is a local sideload build signed with the debug key, not a store release. Device testing status is tracked separately from build success.

### Play the browser build on Android

For a phone playtest over Tailscale, connect both devices to the same tailnet and bind the server to the development machine's Tailscale IPv4 address.

```sh
tailnet_ip=$(tailscale ip -4)
cargo run --locked -p zombiebox-web -- --bind "$tailnet_ip:8765"
```

Open `http://<tailscale-ip>:8765` on the phone, substituting the development machine's Tailscale IP. The phone needs access to TCP port 8765. Runs live in server memory and can be restarted from the playtest screen. The browser build uses its own on-screen controls, WASD or arrow keys, **L** to scavenge, **Space** to fire, and **Escape** to pause.

## Replay the encounter headlessly

```sh
cargo run --locked -p zombiebox-core --example survival -- 42
```

`42` is the world seed. The example replays the original cache and blast encounter used by the headless tests and prints the results after each command.

## Project structure

| Path | What lives here |
| --- | --- |
| [crates/zombiebox-core](crates/zombiebox-core) | Fixed-tick survival rules, world generation, gameplay events, and the headless example |
| [crates/zombiebox-ffi](crates/zombiebox-ffi) | Versioned C interface for native hosts to own and step a Rust run |
| [crates/zombiebox-web](crates/zombiebox-web) | Playtest HTTP host for the Rust simulation |
| [apps/mobile](apps/mobile) | React Native Android app, JNI host, and native build guide |
| [apps/web](apps/web) | Isometric Canvas view, touch controls, and HUD for the browser playtest |
| [docs/architecture.md](docs/architecture.md) | Stack decision, runtime ownership, rendering plan, and device acceptance criteria |
| [docs/world-design.md](docs/world-design.md) | County layout, supply-run design, and progression through hordes and vehicles |

## Run the checks

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
sh scripts/check-native.sh
```

The last command requires a Unix host and a C compiler. See the [mobile development guide](apps/mobile/README.md) for Android setup, APK build and sideload instructions, play controls, and the future iOS port.
