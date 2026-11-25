# Zombiebox project goals

Directional guidance for upcoming milestones. Keep implementation choices flexible—these notes capture player-facing intent, ideas worth exploring, and trade-offs to watch rather than rigid specs. See [product-decisions.md](product-decisions.md) for the milestone checklist and completion checks.

## Action playability and controls

- **Faster, snappier movement.** Tune survivor movement speed (and matching enemy/combat pacing) so maneuvering around houses, kiting threats, and slipping through doorways feels brisk rather than sluggish.
- **Twin-stick firing without mid-screen thumb travel.** Shooting should never require leaving the sticks to tap a separate Fire button in the middle of the screen.
  - **Preferred direction to try first:** tapping the right-stick area fires while dragging aims.
  - **Other mobile twin-stick patterns to keep open:** pushing the right stick past an outer fire ring (auto-fire), release-to-fire, or a fire trigger tucked right beside the right thumb.
  - **Configurable control options:** consider offering multiple selectable twin-stick control schemes in a simple settings menu so players can pick what feels best on their phone.
- **Ergonomic interactions and camera sightlines.** Keep Search/Interact and tool/medkit switching within easy thumb reach without causing accidental misfires. As movement speeds up and ranged enemies arrive, consider subtle camera aim look-ahead so threats stay visible before they attack.

## Audio, haptics, and settings

- **Sound effects across gameplay and UI.** Add responsive audio for UI clicks, weapon fire (pistol, shotgun, rocket launch/blast, axe swing/hit), material impacts (shattering glass, splintering wood, stone/wall thuds), item pickups/switches, medkit use, monster alerts/hits, and player damage.
- **Asset strategy.** Use FOSS/CC0 sound libraries, procedural/synthetic audio, or a blend of both so audio matches the retro Genesis/PS1 feel, stays license-clean, and works fully offline.
- **Touch polish and basic settings.** Consider subtle Android haptic feedback for shots, melee hits, blasts, and damage, paired with a lightweight settings panel (SFX volume/mute and control-scheme selection).

## Residential block and destructible environments

- **Quality before map scale.** Expand Birch Street from one single-story house into a full, high-craft **residential block** before scaling out to a larger city:
  - Multiple **multi-level residential houses** with distinct floor plans, yards, sidewalks, **streetlights**, and lived-in neighborhood props.
  - **Multi-level readability and verticality:** plan clean upper-floor/roof cutaways, smooth stair transitions, and true cross-floor gameplay—monsters follow the player up and down stairs, and the survivor can shoot out of or drop from upper-story windows.
  - **Structural collapse (aspirational):** as destruction matures, explore structural support physics so destroying load-bearing supports causes unsupported upper floors, overhangs, or roofs to collapse inward.
  - **Atmosphere and visibility:** pair streetlights with dusk/night lighting or interior line-of-sight shading (fog of war) so dark corners, lit porches, and peeking through windows matter—and knocking out a streetlight meaningfully darkens its corner.
- **Broadly destructible world.** Aim for nearly everything in the environment to be breakable with intuitive material durability and tool matchups:
  - Light/fragile elements like **windows** shatter in a single axe hit (or gunfire/blasts), opening new sightlines and climbable entry routes.
  - **Doors, furniture, fences, and streetlights** break under sustained melee or gunfire (and doors/broken windows should support opening, closing, or climbing through, not only destruction).
  - **Structural walls** resist light tools (e.g., ~1000 axe hits) but breach quickly under heavy explosives like a rocket launcher.
  - Destruction dynamically updates walkability, line of sight, pathfinding, lighting, sound emission, and saved chunk edits.
- **Toward a full city.** Longer term, grow into a full city map—hand-laid or procedural—only as building templates and block pieces meet the residential quality bar. Moving houses and props out of hardcoded Rust coordinates into lightweight data-driven templates and a destructible material/HP table will make iterating on quality much faster.

## Tools and weapons

- **Distinct survival kit.** Expand beyond the pistol with tools that fill clearly different combat, survival, and breaching roles:
  - **Axe:** close-range melee, rapid window/prop breaching, and a dependable fallback when ammo runs dry.
  - **Pistol:** accurate, low-bulk sidearm.
  - **Shotgun:** close-range spread with strong stopping power / knockback.
  - **Rocket launcher:** explosive area damage and heavy structural breaching (with clear blast feedback so self-damage feels fair).
  - **Medkit:** restores survivor health.
  - Leave room for additional survival tools later (e.g., flashlight, distractions/throwables, crowbar).
- **Switching and ammo.** Support thumb-friendly quick-switching (such as a compact hotbar or quick-slot + dedicated quick-heal), distinct ammo pools/reloading as weapons diversify, and clear equipped/ammo readability on the HUD so running dry on one firearm still leaves melee or secondary options.

## Monster AI and variety

- **Sight- and sound-reactive AI.** Replace omniscient pursuit with awareness states:
  - **Wander / idle** when unaware of the player.
  - **Sight:** detect the survivor via line of sight (blocked by intact walls, floors, and poor visibility).
  - **Sound:** react to gunshots, explosions, breaking windows/walls, and loud movement by moving to investigate the sound's origin.
  - Investigate last-seen or last-heard locations, then settle back into wandering if the trail goes cold.
- **Distinct monster archetypes.** Build recognizable silhouettes and combat behaviors beyond basic shamblers:
  - **Humanoids:** baseline melee threats (with room for speed/toughness variation).
  - **Big bloated things:** heavy, high-HP bruisers (e.g., heavy hits, knockback, or a volatile burst on death).
  - **Shooters:** ranged threats that reward using cover, flanking, and breaking new lines of sight.
  - Leave room for further archetypes as districts expand.
- **Combat readability.** Multi-HP enemies, hit reactions, knockback, and readable attack telegraphs (especially for shooters and heavy attacks) keep touch combat fair and satisfying.

## Supporting notes for future sessions

- **Block objective loop.** As the residential block grows beyond three zombies and one house, define a replayable block objective (e.g., scavenge/clear/extract or return to a safe house) that bridges naturally into the persistent medicine run.
- **Performance and scaling headroom.** Current action AI runs per-tick BFS and rendering uses CPU software pixel fills; budget pathfinding throttling/caching (invalidated on wall/window breaks) and watch physical Android handset frame pacing as multi-level houses, lights, and monster counts grow (GPU rendering via `wgpu` remains an option noted in [architecture.md](architecture.md#platform-and-persistence)).
- **Browser playtest scope.** The web build still runs the separate 20 Hz tile-command runtime; future sessions can either keep it as a lightweight legacy harness or migrate it to `ActionGame` (WASD + mouse) if desktop testing of action features becomes valuable.
