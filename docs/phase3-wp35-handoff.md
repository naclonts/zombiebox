# Phase 3 WP 3.5 handoff

## Generation and ownership

- `world/archetypes/houses/split_level.rs` fits two whole-floor arrangements to the lot, with the taller wing behind or beside the street-level wing. The upper mask overlaps its ground-floor support, and a `StairLink` specifies both endpoints and landings. Wing IDs are carried by the room masks and cells so each roof can cut away independently.
- `world/archetypes/houses/duplex.rs` fits two furnished homes against a continuous party wall. Each home owns a set of rooms, a wing, and a street-facing entry. Arrangement A pairs the doors. Arrangement B offsets one stoop and branches from exterior frontage. The furnishing reachability pass checks each unit independently.
- `world/parcels.rs` ranks frontages by connected road distance from spawn. After selecting the original three starter houses, it fits the wider duplex first and the split-level second on distinct available residential lots. Layout order and compact fallbacks are deterministic. `World::new` streams all five families through the usual county path.

## Review scenes

Use the desktop viewer with `--split-level-lot` or `--duplex-lot`. Both scenes start the survivor at the selected frontage. Omit the fixture flag to explore regular generation.

```sh
cargo run -p zombiebox-render3d --example viewer --features viewer -- --split-level-lot --seed 42
cargo run -p zombiebox-render3d --example preview3d --features gpu -- --scene duplex --seed 63 --mode orbit --size 1280x720
```

The opt-in split-level fixture selects A with north frontage for seed `0` and B with north frontage for seed `42`. The duplex fixture selects A with south frontage for seed `63`. `preview3d --focus upstairs` shows the split-level stair hall. `--focus bedroom` selects a furnished bedroom in either family.
