# Zombiebox

A single-player isometric zombie survival sandbox. In the browser playtest, enter the house on Birch Street, scavenge a pistol, and clear three zombies from the block. Native Android and iOS apps are planned.

## Run the browser playtest

From the repository root, start the dev server with stable Rust 1.87 or newer.

```sh
cargo run --locked -p zombiebox-web
```

Open `http://127.0.0.1:8765`. Follow the lit doorway and stand on the marked crate to scavenge. **Fire** highlights when the pistol has a clear shot. Use the on-screen controls or move with **WASD** or the **arrow keys**, scavenge with **L**, fire with **Space**, and pause with **Escape**.

### Play on Android

For a phone playtest over Tailscale, connect both devices to the same tailnet and bind the server to the development machine's Tailscale IPv4 address.

```sh
tailnet_ip=$(tailscale ip -4)
cargo run --locked -p zombiebox-web -- --bind "$tailnet_ip:8765"
```

Open `http://<tailscale-ip>:8765` on the phone, substituting the development machine's Tailscale IP. The phone needs access to TCP port 8765. Runs live in server memory and can be restarted from the playtest screen.

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
| [apps/mobile](apps/mobile) | React Native Fabric component contract and mobile integration notes |
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

The last command requires a Unix host and a C compiler. See the [mobile development guide](apps/mobile/README.md) for React Native Codegen, Android SDK and emulator prerequisites, and the first native milestone.
