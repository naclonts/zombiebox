# Zombiebox project goals

Directional guidance for upcoming milestones. Keep implementation choices flexible—these notes capture player-facing intent, ideas worth exploring, and trade-offs to watch rather than rigid specs. See [product-decisions.md](product-decisions.md) for the milestone checklist and completion checks.

## Action playability and controls

- **Faster, snappier movement.** Tune survivor movement speed (and matching enemy/combat pacing) so maneuvering around houses, kiting threats, and slipping through doorways feels brisk rather than sluggish.
- **Twin-stick firing without mid-screen thumb travel.** Shooting should never require leaving the sticks to tap a separate Fire button in the middle of the screen.
  - **Implemented schemes to validate and refine on hardware:** Tap Stick (drag to aim, tap to fire, double-tap & hold for continuous fire), Outer Ring auto-fire, and Release-to-fire with center cancel—selectable in Settings.
- **Ergonomic interactions and camera sightlines.** Keep Search/Interact and tool/medkit switching within easy thumb reach without causing accidental misfires. Tune subtle camera aim look-ahead so threats stay visible during faster movement and ranged combat.

## Audio, haptics, and settings

- **Sound effects across gameplay and UI.** Maintain responsive retro audio for UI clicks, weapon fire (pistol, shotgun, rocket launch/blast, axe swing/hit), material impacts (shattering glass, splintering wood, stone/wall thuds), item pickups/switches, medkit use, monster alerts/hits, player damage, and upcoming vehicle sounds (engine RPM, tire skid/gravel, crash/scrape, water/propeller audio).
- **Asset strategy.** Use openly licensed audio where it improves feedback and keep procedural effects where they fit. Audition Josh W. Comeau’s `hi75-soft` for weapon selection and reload button clicks, and consider openly licensed gunshot samples for the pistol and shotgun. Bundle selected sounds for offline play and record their sources and licenses. [Button Sounds playground](https://buttons.joshwcomeau.com/) · [MIT-licensed raw recordings](https://github.com/joshwcomeau/button-sounds/blob/main/docs/raw-audio-files.md)
- **Touch polish and settings.** Pair subtle Android haptic feedback (shots, melee hits, blasts, damage, and future vehicle traction/crash cues) with a lightweight settings panel (SFX volume/mute, control scheme, camera options).

## Residential block and destructible environments

- **High craft as the baseline bar.** Birch Street's three multi-level houses, yards, streetlights, dusk lighting, and destructible materials set the quality bar to carry forward into procedural districts:
  - Multiple **multi-level structures** with distinct floor plans, clean upper-floor/roof cutaways, smooth stair transitions, and true cross-floor combat (monsters following stairs; shooting out of or dropping from upper-story windows).
  - **Broadly destructible world:** fragile elements like **windows** shatter in one axe hit and become climbable; **doors, furniture, fences, and streetlights** break under melee or gunfire (and broken streetlights darken their area); **structural walls** resist light tools but breach under heavy explosives, with unsupported overhangs collapsing inward.
  - Destruction dynamically updates walkability, line of sight, pathfinding, lighting, sound emission, and saved chunk edits.
- **Data-driven templates for scale.** Move building footprints, multi-floor layouts, props, and material/HP definitions out of hardcoded coordinates into reusable, composable templates so residential neighborhoods, downtown blocks, hospitals, police stations, army bases, and woodland cabins can all share the same destructible quality.

## Procedural world and plausible regions

- **Minecraft-style seeded worlds.** Every world is procedurally generated from a seed: the overall terrain, where regions sit relative to each other, road and river networks, each district's street/campus layout, and the individual buildings inside each area all vary from seed to seed. Players should be able to share or replay a seed and get the same county.
- **Rich region & biome roster.** Grow from a single residential block into a full county containing:
  - **Residential neighborhoods:** varied multi-level houses, driveways, yards, fences, and streetlights.
  - **Downtown:** denser, larger-scale multi-story commercial/mixed-use buildings, shops, apartments, alleys, parking lots, and wide intersections.
  - **Hospital:** multi-wing, 4+ story medical complex with varied campus layouts per seed (ambulance bay, lobby, wards, operating rooms, locked pharmacy, button-strip elevators), strong signage/wayfinding, and readable medical lore.
  - **Police station(s):** security-zoned precinct buildings with public lobby, offices, interrogation, barred cells, sallyport, and keycard/breach-gated armories.
  - **Extensible compounds & deferred army base:** build a modular compound/setpiece system (supporting future areas like a **traveling circus** with a Big Top and Ferris wheel), while deferring the **army base** until **tanks and planes** are implemented.
  - **Countryside & woods with cabins:** rural roads, fields/farms, dense woods, and occasional secluded cabins.
  - **Natural terrain:** **mountains** (elevation/ridges/passes), **rivers** (with bridges where roads cross), and **lakes** (shorelines, docks, lakeside cabins).
- **Geographic plausibility.** Procedural layouts must feel believable rather than random:
  - Downtown sits adjacent to residential neighborhoods—never dropped in the middle of deep woods.
  - The hospital connects cleanly to town arterials near downtown/residential areas; police stations sit in or bordering downtown.
  - The army base sits on the outer edge of town with road access and room around it.
  - Country roads, farms, woods, cabins, lakes, and mountains ring the developed area and connect via bridges and mountain passes without stranding required locations.
- **District identity in loot and threats.** Give every area a clear gameplay reason to visit and a distinct tactical profile:
  - **Hospitals:** higher concentrations of **medkits**, **melee zombies**, and **mutated zombies**.
  - **Army bases & police stations:** higher concentrations of firearms/heavy gear and **shooting zombies**.
  - **Downtown / residential / country:** distinct balances of supplies, vehicles, cover density, and threat pressure.
- **Wayfinding in a procedural world.** Because each seed has a new map, help players read the world naturally through road hierarchy (highways -> town streets -> dirt lanes), intersection signage, distinct architecture/lighting, and optional map/compass aids.

## Vehicles and motorcycle riding

- **Long-term vehicle lineup:** **motorcycles**, **jeeps**, **motorboats**, and **planes**—giving land, off-road, water (rivers/lakes), and air (mountains/long-range) traversal across the procedural county.
- **Start with motorcycles.** Prototype motorcycles first to test vehicle feel, physics, camera behavior, and fast chunk streaming. Generate one usable bike near the normal county start and a few more throughout the county from the seed.
- **Semi-realistic dual-stick motorcycle controls (to prototype and tune):**
  - **Left virtual stick:** horizontal input turns the **handlebars**, up applies throttle, and down brakes. Tune the axes together so steering and speed control feel comfortable with one thumb.
  - **Right virtual stick:** controls rider **leaning** (shifting body weight left/right, and potentially forward/back pitch/tuck).
  - Keep exact throttle/brake curves, assist levels, and deadzones flexible—expect to iterate and play-test extensively until the controls feel physical, learnable, and fun on a touchscreen.
- **Physical consequences & mastery:**
  - **Speed-dependent stability:** handlebars and leaning must work in concert with speed.
  - **High-speed handlebar yank (above ~35 mph):** snapping the handlebars hard right or left in one tick at very high speed—regardless of lean—breaks traction or violently upsets the bike, ejecting the survivor off the bike to tumble and take **injury**, while the **unmanned motorcycle skids onward** on its momentum (scraping sparks, knocking props, or plowing into zombies). Gradual steering and leaning, however much of the speed-scaled lock is used, stay recoverable at any road speed.
  - **Low-speed excessive lean (e.g., ~2 mph):** leaning sharply with too little forward speed lacks centrifugal force to hold the bike upright—causing the bike to **tip/fall over** on the spot.
  - **Bike damage:** Crashes and impacts wear down the bike's condition. A bike with zero condition stays where it comes to rest and can no longer be ridden.
  - **Rewarding riding feel:** smooth low-speed handlebar maneuvering, coordinated lean-into-turns at road speed, surface traction differences (asphalt vs gravel/grass), braking/drifting, and mounting/dismounting cleanly.
- **Vehicle camera exploration:**
  - In a fixed isometric camera, mapping "lean left/right" and "turn handlebars left/right" to a stick can feel backwards or confusing whenever the bike heads down-screen or diagonally.
  - **Explore a dedicated riding camera mode**—such as a **behind-overhead / chase-aligned camera** oriented behind the bike's direction of travel so screen-left always equals bike-left, screen-right always equals bike-right, and the player gets longer look-ahead down the road at speed.
  - Compare behind-overhead chase view against fixed isometric (and consider smooth transitions when mounting/dismounting or a toggle in Settings during prototyping).
- **Readable vehicle feedback:** communicate speed, handlebar angle, lean angle, and grip/stability limit clearly through bike sprite/model tilt, dust/skid VFX, subtle HUD/gauge indicators, engine & tire-screech SFX, and haptic rumble before a wipeout.
- **Subsequent vehicles:**
  - **Jeeps:** steadier 4-wheel handling, cargo storage, light off-roading, and durability against bumps/infected.
  - **Motorboats:** buoyant water physics, wake/turn radius, river/lake navigation, and shoreline/dock boarding.
  - **Planes:** takeoff roll, pitch/bank flight controls, landing gear/runway space (army base airstrip or open country fields), high-speed aerial camera/streaming, and dramatic crash consequences.

## Tools and weapons

- **Distinct survival kit.** Maintain clearly differentiated combat, survival, and breaching roles:
  - **Axe:** close-range melee, rapid window/prop breaching, and dependable fallback when ammo runs dry.
  - **Pistol:** accurate, low-bulk sidearm.
  - **Shotgun:** close-range spread with strong stopping power / knockback.
  - **Rocket launcher:** explosive area damage and heavy structural breaching (with fair, readable blast feedback).
  - **Medkit:** restores survivor health.
  - Leave room for additional survival tools later (e.g., flashlight, distractions/throwables, crowbar, repair/fuel tools for vehicles).
- **Switching and ammo.** Keep thumb-friendly quick-switching, quick-heal, distinct ammo pools/reloading, and clear HUD readability—and decide cleanly how weapon access behaves while riding/driving vs on foot (e.g., dismount to fight first, with optional one-handed drive-by fire only if it fits later).

## Monster AI and variety

- **Sight- and sound-reactive AI.** Maintain awareness states:
  - **Wander / idle** when unaware.
  - **Sight:** detect the survivor via line of sight (blocked by intact walls, floors, and poor visibility).
  - **Sound:** react to gunshots, explosions, breaking windows/walls, movement noise, and loud **vehicle engines / crashes** by moving to investigate.
  - Investigate last-seen or last-heard locations, then settle back into wandering if the trail goes cold.
- **Distinct monster archetypes.** Build recognizable silhouettes, audio cues, and combat behaviors tied into district ecology:
  - **Humanoids (melee zombies):** shamblers and brisk runners forming baseline street and interior threats (especially dense in neighborhoods, downtown, and hospitals).
  - **Mutated zombies:** heavy bloated monsters (volatile/high-HP) plus room for additional hospital/late-game mutant variants (e.g., leapers, armored brutes, or caustic spitters) that make medical campuses genuinely dangerous.
  - **Shooters:** ranged armed infected that reward cover, flanking, and breaching new sightlines—concentrated especially in **army bases** and **police stations**.
- **Distinct 3D models for Bloaters and Shooters.** Make Bloaters easy to recognize by their swollen build and readable blast buildup. Give Shooters a recognizable armed stance and a clear aiming animation at normal gameplay zoom.
- **Combat readability.** Multi-HP enemies, hit reactions, knockback, and clear attack telegraphs keep touch combat fair and satisfying.

## Supporting notes for future sessions

- **Performance and scaling headroom.** Keep watching frame pacing on modest physical Android hardware as the world expands from one block to streamed procedural chunks, multi-district buildings, elevation/water, and vehicles. Notice that a **rotating behind-overhead vehicle camera** and high-speed driving put new pressure on the current fixed-angle CPU software renderer—evaluate early whether a rotated software pass or moving to GPU rendering (`wgpu`) is the cleaner path (see [architecture.md](architecture.md#scaling-from-one-street-to-the-county)).
- **Browser playtest scope.** The web build still runs the separate 20 Hz tile-command runtime; future sessions can keep it as a lightweight legacy harness or migrate it to `ActionGame` if desktop testing of procedural worlds and vehicle physics becomes useful.
