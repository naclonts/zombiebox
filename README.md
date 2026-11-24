# Zombiebox

A single-player isometric zombie survival sandbox for iOS and Android. You wake up in a residential neighborhood with the hospital across town. A blast could get you out of the pharmacy with medicine. Can you get home once the street hears it?

This repository starts with a headless Rust encounter, a C interface exercised by a native host, and a React Native game-view specification. The proposed phone client uses Rust for world simulation and rendering, with React Native for menus, inventory, and HUD. The GPU renderer and installable phone apps are upcoming milestones.

## Try the encounter

Run this from the repository root with stable Rust 1.87 or newer.

```sh
cargo run --locked -p zombiebox-core --example survival -- 42
```

The argument is the world seed. The example wakes the survivor at a cache, scavenges a weapon and three charges, and plays four commands against three approaching zombies. It prints the remaining supplies, zombie count, and effects after each command. Reusing a seed and command sequence reproduces the encounter.

## Project structure

| Path | What lives here |
| --- | --- |
| [crates/zombiebox-core](crates/zombiebox-core) | Fixed-tick survival rules, world generation, gameplay events, and the headless example |
| [crates/zombiebox-ffi](crates/zombiebox-ffi) | Versioned C interface for native hosts to own and step a Rust run |
| [apps/mobile](apps/mobile) | React Native Fabric component contract and mobile integration notes |
| [docs/architecture.md](docs/architecture.md) | Stack decision, runtime ownership, rendering plan, and device acceptance criteria |
| [docs/world-design.md](docs/world-design.md) | County layout, supply-run design, and progression through hordes and vehicles |

## Verify the foundation

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
sh scripts/check-native.sh
```

The native check builds the library and compiles a C caller on a Unix host with a C compiler. Mobile Codegen has its own setup under [apps/mobile](apps/mobile).
