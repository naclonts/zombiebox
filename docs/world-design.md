# A county worth learning

Zombiebox draws on the familiar towns, scavenging, and sound-sensitive hordes of a zombie survival game. For the first mobile version, concentrate that tension into a short supply run. Track health and carried supplies. Detailed wounds, hunger, crafting trees, and base construction can wait until the trip itself is worth repeating.

Use a **hand-laid county with seeded local variation**. A returning player should remember the road from the suburbs to the hospital. Which doors open, what the pharmacy still has, and what waits in the ambulance bay can change each run. Cataclysm: Dark Days Ahead uses this kind of local variation, selecting building layouts according to the terrain on the larger map. [Cataclysm map generation](https://docs.cataclysmdda.org/JSON/MAPGEN.html)

The initial Rust scaffold models a county of 32 × 32 chunks, each 16 × 16 tiles. The residential starting area, city center, hospital, farms, and northeastern military base have fixed positions. Roads cross chunk edges. The first seed-dependent details are ground variation and supply caches. The interiors, encounters, and progression below are design targets.

## The first supply run

Wake in a residential block, grab a weapon, and follow the hospital sign to the pharmacy. On the way out, trigger an exploding prop to scatter the group at the door. As the next group converges on the noise, cut through a backyard to reach home with the medicine. Stashing it completes the outing and leaves the survivor free to venture out again. Death ends the run, and the next survivor gets a new seed.

| District | Reason to visit | Choice on the ground |
| --- | --- | --- |
| Residential | Starter tools, food, a place to stash supplies | Search one more kitchen or slip through a yard while the street is clear. |
| City | Shops and concentrated loot | Follow the broad avenue with a clear view or take a shorter alley with a blind corner. |
| Hospital | Reliable medical supplies | Try the exposed ambulance bay or work through a longer side entrance. |
| Farm | Food and fuel | Cross open fields in view of the herd or follow an obscured fence line. |
| Military base | Late-run equipment | Carry a valuable find back to the gate or drop it to make a quicker escape. |

## Generate only the part that benefits from surprise

Author the road connections, district boundaries, landmark entrances, and spawn areas. Choose building templates that fit each lot's road-facing edge. Use separate seed inputs for layout, loot, and encounters so adding a crate cannot move a building. For each candidate seed, check that the starter cache and a public entrance to every landmark are reachable on foot.

Generate a chunk's baseline from its coordinates, the run seed, and the generator version. Persist edits such as opened containers, broken doors, dropped items, and dead enemies. Revisiting a looted kitchen after unloading its chunk must leave it looted. Keep a representative seed fixed for device comparisons and record the seed of every playtest failure.

## Prototype milestones

1. **One block on a phone.** Draw the grid as 2:1 diamonds, with characters anchored at their feet. Map the left thumb to movement in screen space, the right thumb to aiming or attacking, and a context button to searching. Fade a foreground wall when it hides the survivor. A first-time player should be able to loot, aim through a doorway, and recognize the exit during an explosion. Start with silhouettes, foot shadows, a short flash, and a small debris burst.
2. **A persistent supply run.** Connect a residential interior to a hospital entrance. Suspend mid-trip, resume, return home, and verify the same inventory, depleted containers, and local threats. Test across a chunk seam so that loading the next block never duplicates loot.
3. **A county under pressure.** Add the city, farm, and base encounters. Model distant hordes as a population, position, and destination on the road network. Expand into individual zombies near the player and reconcile survivors on departure. Lure a horde across a seam, double back, and count survivors before and after the crossing.
4. **One drivable vehicle.** Reserve road widths, junctions, and parking spaces in the templates from the start. Prototype a single car with enter, drive, park, and exit actions. Stream ahead along its direction of travel. Drive from town to the farm, park across a chunk boundary, reload, and recover the car and its cargo exactly once.
