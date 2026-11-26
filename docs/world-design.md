# A procedural county from a seed

Zombiebox draws on the familiar towns, scavenging, and sound-sensitive hordes of a zombie survival sandbox, expanded into a **seed-driven procedural world** inspired by Minecraft. Entering a seed deterministically generates an entire playable county: its natural geography, the arrangement of its towns and districts, the road and bridge networks linking them, the layout of every neighborhood and compound, and the individual multi-level buildings inside each block.

> **Playable survey.** Choose County atlas on Android or open the browser field atlas. Generator v2 uses one 512 × 512 valley per seed. The atlas shows the road network, building floors, and district ecology. It supports walking and stair traversal. Use seed 42 as the reference scene. [Atlas controls](../README.md#explore-a-procedural-county)

## Regions and natural geography

Every generated world should contain a recognizable mix of developed areas and wilderness, each scaled and laid out uniquely for the seed while keeping the architectural detail and destructibility established on Birch Street:

| Area / Feature | Scale & building variety | Loot emphasis | Infected ecology |
| --- | --- | --- | --- |
| **Residential neighborhood(s)** | Multi-block suburbs with varied multi-level houses (bungalows, duplexes, split-levels, garages), yards, fences, porches, and streetlights. | Starter tools (axe, melee), household supplies, light firearms/ammo, food, and safe-house stash spots. | Balanced baseline mix (shamblers, brisk runners) with manageable street density near the player start. |
| **Downtown** | Denser multi-story urban grid with varied commercial storefronts, offices, apartments, alleys, intersections, and parking lots. | High-value general loot, shops, tools, vehicles parked along streets and lots, and shortcuts through interiors. | Higher overall infected density, tight interior ambushes, and mixed archetypes patrolling streets and blocks. |
| **Hospital** | Large, multi-wing, multi-story medical campus (ambulance bay, lobby/ER, inpatient wards, pharmacy, service corridors, parking) whose footprint and floor plans vary by seed. | Concentrated **medkits** and medical supplies. | Heavy concentration of **melee zombies** and **mutated zombies** (bloaters and future mutant archetypes) stalking corridors and wards. |
| **Police station(s)** | Civic law-enforcement building or small compound (front desk, holding cells, offices, garage/parking, armory) integrated into town. | Firearms, ammunition, tactical gear, and emergency vehicles. | Higher concentration of **shooting zombies** (armed/ranged infected) defending approaches and interiors. |
| **Army base** | Fortified outer-edge compound with perimeter fencing/walls, guard gates, barracks, motor pool, armory, and flat ground/airstrip space; internal layout varies per seed. | Late-run heavy weapons (shotguns, rocket launchers, rifles), ammo reserves, fuel, and military vehicles (jeeps, eventually planes). | High concentration of **shooting zombies** alongside tough perimeter threats. |
| **Countryside, woods & cabins** | Rural roads, open fields/farms, dense forests, dirt tracks, and occasional secluded cabins or barns tucked into the woods. | Rural survival tools, fuel, stashed firearms/supplies, and quiet fallback shelters; docks may spawn motorboats. | Sparser roaming infected, occasional cabin ambushes, and migrating road groups. |
| **Mountains, rivers & lakes** | Ridges and mountain ranges framing the county, winding rivers crossed by road bridges, and lakes with natural shorelines and occasional docks or lakeside cabins. | Natural navigation landmarks, chokepoints (bridges/passes), boat access, and scenic/strategic routes. | Water blocks foot pursuit; bridges and mountain passes create high-tension chokepoints. |

## Plausible procedural layout rules

Procedural generation should never look like random biome soup. A world's macro layout is generated under **geographic and urban-planning plausibility constraints** so towns feel lived-in and readable on sight:

1. **Macro terrain first.** Generate elevation (valleys, rolling hills, mountain ranges), watersheds (rivers flowing along low terrain into lakes or map edges), and forest/open-country zones before placing civilization.
2. **Buildable settlement siting.** Place the town core on relatively flat, dry, contiguous ground—never perched on sheer mountain peaks or submerged in lakes—with arterial roads hugging gentle grades and crossing rivers via bridges.
3. **District adjacency rules.**
   - **Downtown** forms the urban center and is bordered primarily by **residential neighborhoods** rather than sitting isolated in deep woods.
   - **Residential neighborhoods** wrap around downtown (forming natural urban-to-suburban transition) before giving way to **countryside and woods**.
   - **Hospital** sits on a major arterial road near the **downtown–residential seam**, accessible by road and clearly signposted rather than buried in wilderness.
   - **Police station(s)** sit inside **downtown** or along its immediate border with residential blocks.
   - **Army base** sits on the **edge of town / outer perimeter**—separated from quiet residential streets by a rural or industrial buffer, connected by a main highway/arterial road, and sited on flat ground suitable for perimeter defenses and an airstrip.
   - **Woods, cabins, farms, lakes, and mountains** occupy the surrounding country, with cabins tucked along forest clearings, dirt lanes, or lake shorelines.
4. **Connectivity and reachability invariants.**
   - Arterial roads connect residential neighborhoods, downtown, the hospital, police station(s), the army base, and rural country roads into one traversable network.
   - Roads crossing rivers generate bridges; mountain ranges leave traversable passes or skirting highways so required areas are never walled off.
   - Candidate seeds (or generator post-checks) validate that the starter spawn and a public entrance to every required district/landmark are reachable on foot, and that primary road corridors are wide and clear enough for land vehicles.

## Procedural variety at every scale

Variation happens across four nested layers, each driven by deterministic sub-seeds derived from the world seed so tweaking loot tables never shifts roads or building footprints:

1. **World macro plan (seed -> geography & region graph):** Unique arrangement of mountains, rivers, lakes, town placement, district footprints, and highway/arterial road graphs.
2. **District layout (region -> blocks & lots):** Unique street grids, alleys, cul-de-sacs, campus layouts (hospital wings/courtyards, army base gates/barracks/motor pool), and rural tracks.
3. **Building variety (lot -> multi-level structures):** Just as Birch Street mixes distinct multi-level house templates, each district draws from its own pool of seed-varied building layouts, floor counts, materials, interior furnishings, and entry points—scaled appropriately for downtown blocks, hospital wings, police precincts, army barracks, and woodland cabins.
4. **Local dressing, loot & encounters:** Seeded variation of props, lights, vehicles, supply caches, and infected placements weighted by district ecology.

## Navigation and the supply run

Because no two seeds share the same county map, players orient themselves through **world readability** rather than memorizing fixed coordinates:
- **In-world wayfinding:** highway and street signs pointing toward Downtown, Hospital, Army Base, and lakes/country routes, distinctive district silhouettes (streetlights and yards vs tall downtown facades vs hospital signage vs barbed-wire base gates), and an optional discovered-area map/compass as world scale grows.
- **The persistent supply run:** Wake in a procedural residential block, scavenge starter gear, follow roads and landmarks to the hospital (or other target districts) for medkits and supplies, survive district-specific threats, and make it back to a safe house or keep pushing deeper into the county. Track health and carried supplies; defer detailed wounds, hunger, crafting trees, and base construction until the open-world supply and vehicle loop feels compelling. Death ends the run, and the next survivor starts a fresh seed (or optionally replays a shared seed).

## Determinism, chunking, and persistence

- **Deterministic baseline + delta edits:** Generate each 16 × 16 chunk's baseline from `(seed, generator_version, macro_plan, cx, cy)`. Persist player-caused edits—opened doors, shattered windows, breached walls, collapsed overhangs, looted caches, dropped items, dead/surviving threats, and parked or wrecked vehicles—as chunk/world deltas overlaid on the deterministic baseline.
- **Revisit consistency:** Leaving and returning to a chunk must preserve depleted loot, structural destruction, and vehicle locations without duplicating entities or items across chunk seams.
- **Benchmarking seeds:** Keep a fixed reference seed for device performance comparisons and visual regression checks, and record the seed on every playtest bug report.

## Vehicles in the world

The full county is designed for traversal by land, water, and air:
- **Motorcycles (first vehicle milestone):** Fast, nimble two-wheeled road and dirt-track traversal with semi-realistic dual-stick handlebar steering and body-lean physics, high-risk wipeouts, and skidding crash dynamics. Road widths, intersections, driveways, and rural trails should accommodate two-wheelers cleanly.
- **Jeeps (later):** Four-wheel road and light off-road vehicles with cargo capacity and more forgiving handling for hauling supplies between town, country, and the army base.
- **Motorboats (later):** Water traversal along generated rivers and lakes, launching from docks or gentle shorelines to bypass choked roads or reach remote lakeside cabins.
- **Planes (later):** Fixed-wing takeoff and landing from army base airstrips or open rural flats for long-range flight over mountains and across the county.

See [goals.md](goals.md#vehicles-and-motorcycle-riding) for control, physics, and camera exploration details.

## Prototype milestones

1. **Birch Street residential block (implemented on host; handset check pending).** Multi-level destructible houses, streetlights, dusk lighting, expanded tools (axe, pistol, shotgun, rocket launcher, medkit), sight- and sound-reactive AI with multiple infected archetypes, twin-stick firing schemes, and procedural SFX/haptics. Validate frame pacing and touch feel on a modest physical Android handset. [Visual and performance target](product-decisions.md#visual-and-performance-target)
2. **Seed-driven procedural world & plausible regions (implemented).** The default action run and the field atlas open a deterministic county containing mountains, rivers, lakes, woods and cabins, residential neighborhoods, downtown, a hospital, police stations, and an army base. Districts have distinct buildings, loot, and infected. The action run streams nearby chunks and retains world edits in memory.
3. **Motorcycle prototype & vehicle camera.** Add drivable motorcycles with semi-realistic dual-stick controls (left stick handlebars, right stick lean), speed-dependent stability/wipeout/injury/skidding physics, and camera-mode experimentation (e.g., behind-overhead chase view while riding). Stream chunks smoothly ahead of travel and persist parked/wrecked bikes cleanly across chunk boundaries.
4. **Persistent survival & supply run across procedural seeds.** Connect chunk streaming, delta persistence (save/resume, depleted loot, structural destruction, vehicle state), and wayfinding so a player can embark from a procedural residential start to the hospital or army base and return with supplies intact.
5. **Full county under pressure.** Layer in distant road hordes that travel across the road network and resolve into individual infected near the player, plus expanded building templates and mutated/armed enemy variants across every district.
6. **Expanded vehicle roster.** Build out **jeeps**, **motorboats** (navigating procedural rivers and lakes), and **planes** (taking off and landing across airstrips/fields and crossing mountains).
