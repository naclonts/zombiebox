# Zombiebox asset credits

Thank you to the artists and maintainers who make these assets available for reuse.

## Included assets — CC0 1.0 Universal

The following assets are released under [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/) (`CC0-1.0`). Attribution is voluntary and is provided here to thank creators and preserve provenance.

### Kenney — Furniture Kit 2.0

- Creator: [Kenney](https://kenney.nl/)
- Source: [Furniture Kit](https://kenney.nl/assets/furniture-kit)
- License: [bundled upstream license](assets/living-buildings/kenney/furniture-kit/License.txt) (CC0-1.0)
- Source directory: `assets/living-buildings/kenney/furniture-kit/Models/GLTF format/`

| Category | Original filenames |
| --- | --- |
| Bathroom | `bathroomSink.glb`, `bathtub.glb`, `toilet.glb` |
| Bedroom | `bedDouble.glb`, `bedSingle.glb` |
| Storage | `bookcaseClosedDoors.glb`, `bookcaseOpen.glb`, `cardboardBoxClosed.glb`, `cardboardBoxOpen.glb` |
| Seating and office | `chair.glb`, `chairDesk.glb`, `desk.glb`, `loungeSofa.glb` |
| Kitchen | `kitchenCabinet.glb`, `kitchenFridge.glb`, `kitchenSink.glb`, `kitchenStoveElectric.glb` |
| Tables and electronics | `table.glb`, `tableCoffee.glb`, `televisionVintage.glb` |

Twenty source GLBs produce 22 runtime prototypes. Modifications include scaling, coordinate normalization, mesh/material conversion and open/closed variants of the desk and refrigerator.

### kernelCAD — Medical and retail catalog

- Creator: [kernelCAD](https://kernelcad.com/)
- License: CC0-1.0, as declared in each selected model's source metadata.
- Upstream revision: [`fbcae53585f30ab3cbc8f22f1487a40025ca3444`](https://github.com/w1ne/kernelCAD-web/tree/fbcae53585f30ab3cbc8f22f1487a40025ca3444)

| Original model | Vendored source (relative to `assets/living-buildings/`) |
| --- | --- |
| [Diagnostic monitor](https://kernelcad.com/catalog/medical/diagnostic-monitor/) | `medical/kernelcad-source/diagnostic-monitor.kcad.ts` |
| [Hospital bed](https://kernelcad.com/catalog/medical/hospital-bed/) | `medical/kernelcad-source/hospital-bed.kcad.ts` |
| [IV stand](https://kernelcad.com/catalog/medical/iv-stand/) | `medical/kernelcad-source/iv-stand.kcad.ts` |
| [Medical cart](https://kernelcad.com/catalog/medical/medical-cart/) | `medical/kernelcad-source/medical-cart.kcad.ts` |
| [Wheelchair](https://kernelcad.com/catalog/medical/wheelchair/) | `medical/kernelcad-source/wheelchair.kcad.ts` |
| [Assisted checkout till](https://kernelcad.com/catalog/retail-store/assisted-checkout-till/) | `retail/assisted-checkout-till.kcad.ts` |
| [Shopping cart](https://kernelcad.com/catalog/retail-store/shopping-cart/) | `retail/shopping-cart.kcad.ts` |

Seven parametric sources produce eight runtime prototypes. Conversion includes tessellation, scaling, orientation adjustments, simplification where applicable, and separate open/closed checkout till states. The license identification applies to the selected catalog models, not necessarily the kernelCAD software or its dependencies.

### Quaternius — Models distributed via Poly Pizza

- Creator: [Quaternius](https://quaternius.com/)
- Distribution: [Poly Pizza](https://poly.pizza/)
- License: CC0-1.0, as recorded for each model in the conversion manifest and prop asset catalog.
- Source directory: `assets/living-buildings/poly-pizza/`

| Original model | Source file |
| --- | --- |
| [Car](https://poly.pizza/m/Cz6yDaUcM9) | `car.glb` |
| [Police Car](https://poly.pizza/m/BwwnUrWGmV) | `police-car.glb` |
| [Crate](https://poly.pizza/m/3OEFd1AWfa) | `crate.glb` |
| [First Aid Kit](https://poly.pizza/m/Hp80p6148W) | `first-aid-kit.glb` |

Four source GLBs produce four runtime prototypes, including simplification of the cars and crate.


### vinish.dev — Zombie models

- Creator: vinish.dev
- Source: [Free Zombie 3D Models](https://vinish.dev/free-3d-models/zombies)
- License: [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/) (`CC-BY-4.0`)

| Original model | Planned Zombiebox archetype |
| --- | --- |
| Zombie Walker | Shambler |
| Zombie Runner | Brisk |
| Bloated Zombie | Bloater |
| Zombie Soldier | Shooter |

Example attribution: "Zombie Walker" by vinish.dev (https://vinish.dev/free-3d-models/zombies), CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Modified by Zombiebox: mesh simplification and LOD generation.

When importing these assets, move the relevant entries to the included-assets section, record actual paths and modifications, verify the exact source license, and retain supplied copyright and attribution notices (including embedded GLB credits). Include the creator, source, license link, change notice and other required notices with source and compiled distributions. CC BY 4.0 does not require the game code to use that license; its conditions continue to apply to the covered assets and adaptations. Do not impose additional restrictions on recipients' exercise of their licensed rights.

Other candidate assets mentioned in the prop asset catalog are not credited as included unless actually imported.

## First-party code and other dependencies

This file documents asset provenance; it does not establish a license for Zombiebox's first-party code or artwork. Third-party software dependencies retain their own licenses and notices.

When adding or replacing an external asset, update this file and the applicable asset manifest in the same change. Verify that the exact model's terms allow public redistribution of source files, modification and commercial use before importing it.

This game uses AI-generated code. Thank you to humanity for making this possible.

