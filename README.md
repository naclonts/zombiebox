# Zombiebox

A single-player isometric action-adventure and zombie survival sandbox built for Android. The local app plays in landscape with twin-stick controls across a dusk-lit, destructible, multi-level Birch Street residential block: scavenge tools, firearms, and medkits across three two-story houses and surrounding yards, smash windows or breach walls, and clear sight- and sound-reactive infected threats. The legacy browser playtest remains available, and an iOS app is a future platform target.

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

The app runs offline in landscape. Drag the left stick to move (light tilt walks quietly without footstep noise) and use the right stick to aim and fire with your chosen scheme in **Settings** (**Tap Stick** by default: drag to aim, tap to fire, double-tap & hold for continuous fire; **Outer Ring** auto-fire; or **Release** to fire with center-cancel). Use the bottom hotbar to switch between the **Axe**, **Pistol**, **Shotgun**, and **Rocket Launcher**, tap **+ MED** for quick-heal, and tap the contextual interact prompt (or scene near a prompt) to search caches, open/close doors, vault broken windows, drop from 2F windows, or take stairs. Procedural retro SFX, Android haptics, and aim look-ahead are configurable in **Settings**.

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
| [crates/zombiebox-render](crates/zombiebox-render) | Native isometric rendering, Birch Street art, character animation, and combat effects |
| [crates/zombiebox-web](crates/zombiebox-web) | Playtest HTTP host for the Rust simulation |
| [apps/mobile](apps/mobile) | React Native Android app, JNI host, and native build guide |
| [apps/web](apps/web) | Isometric Canvas view, touch controls, and HUD for the browser playtest |
| [docs/product-decisions.md](docs/product-decisions.md) | Product checklist, visual/performance targets, and milestone completion checks |
| [docs/goals.md](docs/goals.md) | Directional guidance for upcoming gameplay, controls, environment, AI, and audio goals |
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
