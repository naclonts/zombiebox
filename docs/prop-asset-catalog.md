# Living buildings prop asset catalog

Keep the selected source files and converted prototypes together under `assets/living-buildings/` in Git so a checkout has everything needed to use or rebuild the meshes. The `converted/` folder contains the indexed meshes and a manifest with provenance, source and output hashes, conversion settings, triangle counts, and measured bounds. When updating a source or conversion setting, regenerate the meshes and include the updated manifest in the same change. Model review and final `PropDef` fit remain part of Phase 2. [Asset folder](../assets/living-buildings/) · [Manifest](../assets/living-buildings/converted/manifest.json) · [Conversion and acceptance checks](phase2-implementation-plan.md#offline-prototype-conversion)

## License list

| Source | License | Evidence |
| --- | --- | --- |
| Kenney Furniture Kit 2.0 | CC0-1.0 | Publisher listing and bundled license. [Furniture Kit](https://kenney.nl/assets/furniture-kit) · [License.txt](../assets/living-buildings/kenney/furniture-kit/License.txt) |
| kernelCAD medical kit | CC0-1.0 | Individual model pages and source metadata. [Medical kit](https://kernelcad.com/catalog/medical/) · [Pinned upstream sources](https://github.com/w1ne/kernelCAD-web/tree/fbcae53585f30ab3cbc8f22f1487a40025ca3444/catalog/kits/medical) |
| kernelCAD retail candidates | CC0-1.0 per selected model | [Assisted Checkout Till](https://kernelcad.com/catalog/retail-store/assisted-checkout-till/) · [Shopping Cart](https://kernelcad.com/catalog/retail-store/shopping-cart/) |
| Quaternius models on Poly Pizza | CC0-1.0 per selected model | Individual model pages in the exterior and small props table. |
| Tevy IV stand and wheelchair | CC0-1.0 per selected model | [Abandoned IV Stand](https://tevy.dev/models/abandoned-iv-stand) · [Wheelchair](https://tevy.dev/models/wheelchair) |
| Grab3D hospital bed, medical cart, and monitor | CC0-1.0 per selected model | Individual model pages in the medical equipment section. |

CC0 permits copying, modification, commercial use, and redistribution of the covered models without mandatory attribution. Record creator and source for provenance. Poly Pizza labels each model individually. Prefer verified CC0 picks. CC BY 4.0 can also support commercial redistribution and adaptation when the release carries the required credit, license link, and change notice. Assess any other license before selecting the model, and use sources whose terms cover modification, public source distribution, and paid releases. [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) · [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)

## Residential, office, and shared fixtures

The repository contains the selected Kenney Furniture Kit GLBs under `kenney/furniture-kit/Models/GLTF format/`. The source meshes use indexed triangles and material colors. Filenames below are relative to that folder. The original upstream archive SHA-256 is `e67652d0932cee41683f74711c03d3e192a2af9979ef8e6b237711f5482d46b0`.

| Use | Source GLB | Planned adaptation |
| --- | --- | --- |
| Living-room sofa and coffee table | `loungeSofa.glb`, `tableCoffee.glb` | Normalize for room placement and use a subdued fabric palette. |
| Single and double beds | `bedSingle.glb`, `bedDouble.glb` | Fit the declared bed footprints. |
| Dining chairs and tables | `chair.glb`, `table.glb` | Reuse for breakrooms and generic dining furniture. |
| Kitchen base cabinet, sink, and stove | `kitchenCabinet.glb`, `kitchenSink.glb`, `kitchenStoveElectric.glb` | Use as straight modules where they fit. Generate fitted corner and long counter runs. |
| Searchable refrigerator | `kitchenFridge.glb` | Closed and opened prototypes share a pivot. Check the swing of both doors against the interaction envelope. |
| Toilet, bathtub, and sink | `toilet.glb`, `bathtub.glb`, `bathroomSink.glb` | Reuse in residential and public restrooms as appropriate. |
| Living-room television | `televisionVintage.glb` | Tag the screen surface for the powered or emissive treatment. |
| Office desk and chair | `desk.glb`, `chairDesk.glb` | Closed and opened desk prototypes share a pivot. Check the extended drawer against approach clearance. |
| Shelves and generic cabinet | `bookcaseOpen.glb`, `bookcaseClosedDoors.glb` | Reuse for rooms and review the cabinet silhouette for a lockable storage variant. |
| Stockroom and household boxes | `cardboardBoxClosed.glb`, `cardboardBoxOpen.glb` | Use as existing paired states where a cardboard box suits the room. |

The first five samples have these source measurements after applying their authored node transforms. Extents are horizontal X × horizontal Z × vertical Y in GLB units. The converted furniture uses a common 2× source scale and the manifest stores those authored bounds. At runtime, indoor furniture and its collision bounds use a further 0.6× scale to match the 1.05-unit survivor. Check the final footprint in the gallery.

| Source GLB | Source extents | Source triangles |
| --- | --- | ---: |
| `loungeSofa.glb` | 0.980 × 0.410 × 0.460 | 128 |
| `bedSingle.glb` | 0.571 × 1.125 × 0.375 | 214 |
| `toilet.glb` | 0.313 × 0.477 × 0.451 | 230 |
| `kitchenFridge.glb` | 0.430 × 0.292 × 0.920 | 250 |
| `desk.glb` | 0.734 × 0.392 × 0.384 | 198 |

## Grocery and fit-to-room pieces

Use modular checkout counters and shelf runs sized by the furnishing grammar. Reuse the shelf geometry for pharmacies with different contents. Source a standalone register or shopping cart only if it fits the grounded proportions and palette. Kenney Mini Market and Retro Urban are excluded from the shortlist because of their cartoon styling.

The converter exports both retail candidates from the pinned upstream parametric sources under `retail/`. The two till states share a pivot. The shopping cart keeps its wire basket and is a candidate for sparse placement pending the gallery and mobile budget checks. [Pinned retail kit](https://github.com/w1ne/kernelCAD-web/tree/fbcae53585f30ab3cbc8f22f1487a40025ca3444/catalog/kits/retail-store)

| Candidate | Staged source | Published triangles | Planned adaptation |
| --- | --- | ---: | --- |
| [kernelCAD Assisted Checkout Till](https://kernelcad.com/catalog/retail-store/assisted-checkout-till/) | `assisted-checkout-till.kcad.ts` | 1,076 | Mount on a modular checkout lane and evaluate the open and shut drawer presets. |
| [kernelCAD Shopping Cart](https://kernelcad.com/catalog/retail-store/shopping-cart/) | `shopping-cart.kcad.ts` | 1,932 | Review simplification before placing carts in groups. |

Generate privacy curtains, floor rugs, cell bars, and pixel-relief signage. Fit kitchen counters and aisle endcaps to the room. Review generic cabinet geometry for a secure locker and use a parametric cabinet where the fit or opening motion requires it. Rooftop HVAC and additional clinical fixtures remain candidates for individual sourcing as needed.

## Medical equipment

The five kernelCAD models were converted from the `.kcad.ts` sources under `medical/kernelcad-source/`. The offline script adapts catalog metadata and parameters in a temporary copy for kernelCAD 0.15.0 and uses polygon cylinders to honor the authored segment counts. Each model uses a uniform scale for a provisional in-game size.

| Use | Source | Planned adaptation |
| --- | --- | --- |
| Patient ward bed | [Hospital bed](https://kernelcad.com/catalog/medical/hospital-bed/) · `hospital-bed.kcad.ts` | Check against bed footprint and bedside approach. |
| Mobility equipment | [Wheelchair](https://kernelcad.com/catalog/medical/wheelchair/) · `wheelchair.kcad.ts` | Check the authored wheel and backrest alignment before placement. Compare the Tevy model if the silhouette reads poorly. |
| Ward and emergency bays | [IV stand](https://kernelcad.com/catalog/medical/iv-stand/) · `iv-stand.kcad.ts` | Preserve the stand silhouette at small scale. |
| Ward vital signs | [Diagnostic monitor](https://kernelcad.com/catalog/medical/diagnostic-monitor/) · `diagnostic-monitor.kcad.ts` | Reserve a screen material for emissive treatment. |
| Ward and emergency storage | [Medical cart](https://kernelcad.com/catalog/medical/medical-cart/) · `medical-cart.kcad.ts` | Evaluate fit in the ward and emergency bay. |

Tevy publishes alternate CC0 GLBs with LOD0, LOD1, and LOD2. The IV stand LOD2 has 672 triangles and the wheelchair LOD2 has 5,292. Both use compressed geometry and textures. Evaluate them if the converted kernelCAD alternatives fail silhouette or clearance review. [Abandoned IV Stand](https://tevy.dev/models/abandoned-iv-stand) · [Wheelchair](https://tevy.dev/models/wheelchair)

These Grab3D downloads remain fallback candidates. Each requires texture baking and simplification before it can fit the prototype format and budget.

| Source | Download filename | Source triangles |
| --- | --- | ---: |
| [Hospital Bed](https://grab3d.com/models/furniture/hospital-bed/) | `hospital-bed-light.glb` | 13,736 |
| [Medical Cart](https://grab3d.com/models/furniture/medical-cart/) | `medical-cart.glb` | 13,996 |
| [Patient Monitor](https://grab3d.com/models/electronics/patient-monitor/) | `patient-monitor.glb` | 19,324 |

## Exterior and small props

The four GLBs under `poly-pizza/` have converted prototypes. Quaternius authored them and the individual listings specify CC0-1.0. Both cars and the crate were simplified.

| Use | Exact source | Download filename | Source triangles | Review |
| --- | --- | --- | ---: | --- |
| Parked car | [Car](https://poly.pizza/m/Cz6yDaUcM9) | `car.glb` | 2,954 | Check repeated placements on mobile. |
| Parked cruiser | [Police Car](https://poly.pizza/m/BwwnUrWGmV) | `police-car.glb` | 3,232 | Check silhouette and color contrast at gameplay zoom. |
| Generic stockroom storage | [Crate](https://poly.pizza/m/3OEFd1AWfa) | `crate.glb` | 784 | Compare with the Furniture Kit boxes. |
| Clinical dressing | [First Aid Kit](https://poly.pizza/m/Hp80p6148W) | `first-aid-kit.glb` | 268 | Check silhouette and scale on a shelf or counter. |

The [kernelCAD military wooden crate](https://kernelcad.com/catalog/containers/ammo-crate/) is listed as CC0-1.0 for future Army Base sourcing. Army Base asset work remains deferred.
