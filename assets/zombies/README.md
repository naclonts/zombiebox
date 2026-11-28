# Zombie character assets

The `converted/` directory contains five skinned character archetypes at four triangle budgets. Meshes use local coordinates with +Y up, +Z forward, ground at Y=0, and a normalized 1 m standing height. Geometry color, including source base-color textures, is baked into sRGB RGBA8 vertex colors. Meshes retain four skin influences per vertex and use the source skeleton joint order.

| LOD | Requested triangles |
| --- | ---: |
| 0 | 10,065 |
| 1 | 4,973 |
| 2 | 1,836 |
| 3 | 632 |

The `*.zbzs` format has a 20-byte little-endian header (`ZBZS`, version, vertex count, index count, reserved), followed by 44-byte indexed vertices (`position f32x3`, `normal f32x3`, `color u8x4`, `joint indices u16x4`, `normalized weights u16x4`) and `u32` triangle indices. `*.zbsk` contains the ordered joint hierarchy, normalized bind-local matrices, and inverse-bind matrices. JSON sidecars and `manifest.json` retain source hashes, output hashes, counts, bounds, and animation clip names.

## Attribution

The five source models are **Bloated Zombie, Runner Zombie, Soldier Zombie, Walker Zombie, and Woman Zombie** by **vinish.dev**, from [Free 3D Models](https://vinish.dev/free-3d-models/), licensed under [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/). The original GLBs were supplied from the user's Downloads directory and are not copied into this repository. Their SHA-256 hashes and original filenames are recorded in `converted/manifest.json`.

These models were modified by reducing them into four LODs, baking base-color textures and factors into vertex colors, normalizing scale/origin, and converting to Zombiebox's ZBZS/ZBJS binary formats. The LOD meshes are approximate; the converter welds duplicate GLB seam vertices for simplification and uses spatial clustering on the most disconnected low-detail meshes. Review LOD3 silhouettes at gameplay scale before further use.

## Regeneration

Place the five named GLBs in `~/Downloads`, then run `python3 scripts/convert_zombie_lods.py` with NumPy, SciPy, Pillow, and fast-simplification available. The original runner duplicate `zombie-runner(1).glb` is identical and intentionally not converted a second time.
