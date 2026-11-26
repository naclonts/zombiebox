# 2.5D Isometric Reference Baselines (`zombiebox-render`)

* **Baseline Commit:** `daeff4a`
* **Reproduction Command:** `cargo run --locked --offline --release -p zombiebox-render --example preview -- --baselines docs/baselines/iso-2p5d`

| File | Scenario | Seed | Player Pos (x, y) | Floor | Resolution | Notes |
|---|---|---:|---|---:|---|---|
| `birch-street-exterior.png` | Birch Street Exterior | 42 | (128.0, 262.0) | 0 | 640×360 | Dusk spawn on Birch Street, exterior houses & 12 infected |
| `birch-street-interior.png` | Birch Street Interior (F0) | 42 | (128.0, 258.3) | 0 | 640×360 | F0 living room cutaway, shattered south window (Some(BrokenWindow)), pistol cache |
| `birch-street-upstairs.png` | Birch Street Upstairs (F1) | 42 | (130.0, 259.3) | 1 | 640×360 | F1 cutaway aiming out shattered window (Some(BrokenWindow)), 1 F0 yard threats below |
| `birch-street-combat.png` | Birch Street Combat | 42 | (136.5, 247.1) | 0 | 640×360 | Broken streetlight (Some(BrokenStreetlight)), 3 breached wall tiles, active rocket blast/telegraphs (1) |
| `county-s42-spawn.png` | County Spawn | 42 | (354.0, 163.0) | 0 | 640×360 | Default procedural residential spawn |
| `county-s42-downtown.png` | County Downtown | 42 | (284.0, 233.0) | 0 | 640×360 | Downtown commercial/apartment entrance |
| `county-s42-hospital.png` | County Hospital | 42 | (213.0, 267.0) | 0 | 640×360 | Hospital complex entrance |
| `county-s42-police.png` | County Police | 42 | (354.0, 201.0) | 0 | 640×360 | Police station entrance |
| `county-s42-army-base.png` | County Army Base | 42 | (410.0, 343.0) | 0 | 640×360 | Fortified army base entrance |
| `county-s42-woods-cabin.png` | County Woods Cabin | 42 | (96.0, 225.0) | 0 | 640×360 | Secluded woodland cabin entrance |
| `county-s42-river-bridge.png` | County River Bridge | 42 | (148.0, 265.0) | 0 | 640×360 | River bridge crossing over open river channel |
