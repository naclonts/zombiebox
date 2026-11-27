# Motorcycle implementation plan

Add drivable motorcycles to the real-time county on Android and in the browser action build. Each county seed generates a small roster, including one usable bike near the residential start. Ride across districts, recover a parked or tipped bike on a return visit, or look for another if a crash disables it. The shared vehicle code owns identity, occupancy, and run state. Each vehicle kind owns its input schema, physics, and presentation data. [Product checklist](product-decisions.md#planned-milestones-and-completion-checks) · [Vehicle goals](goals.md#vehicles-and-motorcycle-riding)

## Prototype gameplay

| Area | Starting behavior |
| --- | --- |
| Availability | Seed one usable bike within a short walk of the normal county start and a few more in accessible spots around the county. Use Birch Street for on-foot regression and a bike fixture for tests. |
| Touch | Left stick X steers the handlebars. Left stick up adds throttle, down brakes, and center coasts. Holding down after a stop pushes the bike backward at walking pace with no engine load. Right stick X shifts the rider's weight. Use raw analog magnitude before aim-specific processing. The Android acceptance run uses the built-in touchscreen on a midrange Samsung handset. |
| Browser keyboard | A/D steer, W/S accelerate and brake (hold S at a stop to push backward), and ←/→ lean. E uses the contextual action. This layout allows throttle, steer, and lean together. |
| Interact | Mount an operative bike from nearby with clear same-level access, including while it is rolling or skidding. Offer dismount at rest or while moving when a clear standing position beside the bike exists; a moving dismount leaves the bike coasting. Offer **Right bike** for a tipped bike with condition remaining. |
| Riding and recovery | Keep both thumbs available for driving. Hide weapon and medkit actions while mounted. After a crash, show the rider's tumble or stumble, then restore on-foot controls after recovery. Crashes and impacts reduce the bike's condition. A bike with zero condition stays visible and inoperable. A moving dismount preserves the bike's momentum so it can be caught and remounted. |
| Camera | Default to a behind-overhead view aligned to bike heading. Offer fixed isometric as a mounted comparison and restore the selected on-foot view after exit. |
| Atlas | Open the atlas as usual. Require the player to stop and dismount, or finish crash recovery, before teleporting within the same run. The bike stays in the county. |

## Core types and contracts

Put the vehicle registry under `ActionGame`. `CountyStage` owns active geometry and infected streaming today. Keep one authoritative record per vehicle ID in the live run and derive its current chunk from position. For the prototype roster, iterate one ordered map and filter by active bounds. This handles seam crossings without moving ownership between chunk lists and leaves room for a derived chunk index when the roster grows. [Action runtime](../crates/zombiebox-core/src/action/mod.rs) · [County streaming](../crates/zombiebox-core/src/action/county.rs)

The signatures below show the intended boundaries. Use the existing spatial units, vector types, `BodyState`, and `DT`.

```rust
// world/vehicles.rs
pub(crate) struct MotorcycleSpawn {
    pub slot: u8, // stable within the generated county
    pub tile: Position,
    pub heading: f32,
}
pub(crate) fn motorcycle_spawns(
    world: &World,
    start: Position,
    starter_is_clear: impl FnMut(&MotorcycleSpawn) -> bool,
) -> Vec<MotorcycleSpawn>;

// action/vehicles/mod.rs
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct VehicleId(pub u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlMode { OnFoot, Motorcycle }
pub enum RideInput { Motorcycle(MotorcycleInput) }
enum VehicleState { Motorcycle(MotorcycleState) }
pub enum VehicleView { Motorcycle(MotorcycleView) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MountedHud {
    pub control_mode: ControlMode,
    pub speed_mph_x10: u16,
    pub grip_use: u8,
    pub engine_load: u8,
    pub condition_pct: u8,
}

struct Vehicles {
    by_id: BTreeMap<VehicleId, VehicleState>, // stable iteration by ID
    mounted: Option<VehicleId>,             // sole source of rider-bike occupancy
}

// action/vehicles/motorcycle.rs
pub struct MotorcycleInput {
    pub steer: f32,    // [-1, 1], positive is rider-right
    pub throttle: f32, // [0, 1]
    pub brake: f32,    // [0, 1]
    pub lean: f32,     // [-1, 1], positive is rider-right
}
enum MotorcyclePhase { Parked, Rolling, Skidding, Tipped }
struct MotorcycleState {
    body: BodyState,   // ground contact, world velocity, yaw, pitch, roll, support
    phase: MotorcyclePhase,
    condition: u8,     // 0..=100, zero disables powered motion
    steer_angle: f32,
    rider_lean: f32,
    traction_use: f32,
    // Include the minimal prior steer/stability and skid state needed by the next tick.
}

// Add ride: Option<RideInput> to ActionInput, defaulting to None.
// Add mounted: Option<MountedHud> to ActionHud.
impl ActionGame {
    pub fn step(&mut self, input: ActionInput); // keep the fixed-step entry point
    pub fn control_mode(&self) -> ControlMode;
    pub fn mounted_vehicle(&self) -> Option<VehicleId>;
    pub fn vehicle_views(&self) -> impl Iterator<Item = (VehicleId, VehicleView)> + '_;
}

// Motorcycle-specific fixed-tick integration and contact response.
fn step_motorcycle(
    bike: &mut MotorcycleState,
    input: Option<MotorcycleInput>,
    world: &dyn SpatialWorld,
    contacts: &[VehicleContactBody],
    tuning: &MotorcycleTuning,
) -> MotorcycleStep;
```

`MotorcycleView` exposes a read-only pose, phase, steer angle, lean, speed, and condition. Derive operability from `condition > 0` so a disabled bike can still be in the `Skidding` phase. `VehicleContactBody` is a read-only infected candidate with ID and continuous body bounds. `MotorcycleStep` reports contact target, point, normal, pre-impact speed, and the optional wipeout cause. `ActionGame` applies damage, noise, occupancy changes, and rider recovery using those results. `None` input advances an unmanned skid. Pass neutral `MotorcycleInput` for a mounted bike with missing or invalid input. For the interaction pulse, accept only a sample whose schema matches the mode at the start of that tick. Future vehicle kinds can add their own `RideInput`, `VehicleState`, and `VehicleView` variants, with aircraft and boats free to use different axes, support rules, and camera targets.

## Seeded placement

Generate a small baseline roster once per new county run. Start with four bikes as a tuning value. Reserve slot zero for the starter bike and search outward from the actual walkable start returned by `CountyStage::new`. Prefer a clear shoulder or driveway along a short walkable route. Extend the deterministic search along the connected starter road if nearby spots fail clearance. Choose the other sites from seed-ranked roadside candidates in distinct parts of the county, such as a town edge and rural road approaches. Require a dry exterior surface, room to stand beside the bike, and a clear way onto the road. Use a dedicated seed hash domain and coordinate tie-breaks for stable results.

Use `World::tile_at` for generated ground and feature checks and the existing terrain/support queries for slope and height. Pass a starter clearance check from `ActionGame::county_with_world` so the search can reject candidates blocked by materialized props or starter loot and continue to the next one. For distant sites, check the exterior tiles under the bike and adjacent mounting positions against building footprints and blocking features. Verify exact mounting space when exercising the generated counties. Initialize `ActionGame`'s registry once with each slot's stable ID, baseline pose, and full condition. Atlas deployment changes the player destination after the roster is built. Derive chunk membership from each bike's live position. [World baseline](../crates/zombiebox-core/src/world.rs) · [County start](../crates/zombiebox-core/src/action/county.rs)

## Gameplay and simulation

1. **Interact with nearby bikes.** Add `Mount`, `Dismount`, and `RightBike` to `InteractAction` and `InteractPrompt`. Preserve existing on-foot prompt priority and consider nearby bikes when existing prompts have no candidate. Use the same candidate function for HUD and execution, revalidating condition, support, distance, clearance, and speed at the tick that consumes Interact. A failed dismount leaves the rider seated.
2. **Route each fixed tick by authoritative control mode.** Keep lifecycle timers running. Preserve the existing on-foot tick order. When mounted, advance motorcycle movement, adopt the new seat pose in the player body for AI, hits, visibility, and streaming, and use Interact only for dismount. Advance unoccupied skidding bikes on either path. After mount, dismount, or ejection, suppress inputs carried over from the prior mode, including old fire, interact, aim, and held throttle. A new action requires a fresh input sample for the resulting mode. Finish with the existing projectile, collapse, noise propagation, AI, outcome, and visibility stages. Clear occupancy in the same tick on death.
3. **Tune a simple speed-dependent handling model.** Keep continuous XY momentum and support height. Use throttle, braking, and drag to update longitudinal speed. Once stopped, a held brake becomes a low-speed leg push in reverse, capped near 2 mph and steered with signed bicycle kinematics; throttle cancels reverse and forward braking always takes priority. Reduce maximum handlebar angle as speed rises and scale yaw rate with forward motion. With a forward vector `(cos(yaw), sin(yaw))`, use `(-sin(yaw), cos(yaw))` for bike-right. Estimate lateral demand from `speed × abs(yaw_rate)` and compare it to the supporting surface's grip. Compare the desired signed lean for that corner to actual rider lean. Allow assisted steering at walking speed and reward matching lean to the turn at road speed. Limit how quickly the velocity vector follows yaw when grip runs out. Track sustained overload over a few ticks for warning feedback, and detect an abrupt raw steer change before smoothing can hide it. Keep the curves and deadzones in `MotorcycleTuning`. Start with a tunable display conversion of 2.24 mph per world unit per second, giving test speeds of about 0.89 and 17.9 world units per second.
4. **Sweep the bike against county geometry and infected.** Start with the swept spatial body solver and a conservative bike footprint. Sample support at the bike's contact height so the bridge deck is drivable above submerged terrain. Block deep water, steep unsupported terrain, stairs, and window traversal. Disable doorway assist and vaulting. Sweep infected within the same 3D travel bounds and cap the sweep at the first blocking world contact. If needed, expose that first contact's fraction and normal from the spatial solver. Resolve contacts in travel order and use entity ID to break ties for infected. Read pre-impact motion to determine speed loss, deflection, damage, and ejection. Add a vehicle impact damage kind to the existing material damage path, and route damage events and AI noise through the existing systems. Give hard obstructions enough resistance to stop a skid, and make light props and infected valid impact targets. Apply each impact once per contact episode so a resting overlap does not repeatedly deal damage.
5. **Resolve falls and skids independently.** Sustained extreme lean near 2 mph tips the bike from near its current location and gives the rider a short stumble. A sharp, unmatched steer near 40 mph ejects the rider and starts a separate motorcycle skid on the current momentum. Reuse the player body sweep for the tumble. Track a pending injury in the rider's recovery state and consume it at the first ground or solid impact through the existing health and `PlayerHurt` flow. Use one crash injury calculation for that impact, including any drop, then let the recovery timer expire before restoring full controls. Add discrete `VehicleCrash`, `VehicleSkidStarted`, and `VehicleImpact` action events carrying vehicle ID and world position. Feed throttled engine and impact noise into the existing AI noise path. [Spatial integration](../crates/zombiebox-core/src/spatial/kinematics.rs) · [Terrain and support](../crates/zombiebox-core/src/spatial/action_adapter.rs) · [Infected damage](../crates/zombiebox-core/src/action/ai.rs)
6. **Apply bike damage.** Use a tunable condition loss for each distinct fall or impact. Base it on severity, including normal impact speed and obstacle type. Apply the loss with saturating subtraction and charge each contact once. A low-speed tip should leave a healthy bike recoverable. A severe hit or repeated moderate hits can consume the remaining condition. When a mounted bike hits zero, end powered motion, release the rider through the crash path, and preserve the bike's momentum for its remaining skid. A settled wreck stays visible at its final pose. Require positive condition for mounting and righting.

## Streaming and run persistence

- Start with the existing player-centered 7 × 7 simulation window of 16-tile chunks and the shared renderer's smaller mesh window. The mounted seat moves the existing focus across seams. Check the predicted sweep against collision bounds and the sightline against the mesh window at maximum prototype speed, including an ejection or skid. Record window rebuild and mesh upload costs before tuning either radius. If the margin falls short, prepare the next forward strip using existing materialization, then consider it queryable once the corresponding geometry is ready. Use the last valid support if movement would otherwise reach unavailable collision geometry.
- Resolve any moving bike through its skid within the loaded collision margin. Choose maximum skid travel and recovery budgets that fit this margin even as the rider moves away. A stationary bike outside the active window remains in the registry and costs no physics ticks. For revisits, filter by live world position and derive the chunk using the existing world-to-tile convention. Keep draw and simulation queries keyed by ID. When an original spawn chunk becomes active, show the bikes currently there from the registry.
- Same-run teleports, surface recreation, and pause/resume retain the registry with the existing `ActionGame`. A fresh run regenerates the roster for its seed at full condition, including when replaying a seed. Durable run saves remain part of the supply-run milestone. That save can capture stable IDs, kind, pose, motion, phase, condition, occupancy, and the transient state needed to resume a crash at a tick boundary. [Save architecture](architecture.md#platform-and-persistence) · [World milestones](world-design.md#prototype-milestones)

## Camera, controls, and frame transport

- **Shared renderer.** Extend `DynamicSceneState` with vehicle views and use the supplied cruiser mesh for the first rendered prototype. Register the four parts as instanced `MeshBuffer` prototypes. Draw the rider once in a seated pose that follows the bike's heading and lean, then return the rider to the existing actor path on ejection. Apply visibility and frustum rules to the bike and use existing effects to show slip, impacts, and the disabled state.
- **Mounted camera.** In `Presentation::prepare`, derive a mounted camera target from the bike pose, yaw, and longitudinal speed. Add a heading-following override in `CameraRig` using elevated chase geometry, shortest-angle damping, speed-based look-ahead, and the existing obstruction cast. Keep the user's foot camera selection available for exit. Suppress mouse-look and right-stick look while mounted, including in the isometric comparison. Reset interpolation and input history on teleport or run replacement. Test left/right alignment with the bike facing all four cardinal directions and check that rotating the camera preserves room visibility.
- **Input adapters.** Map normalized raw screen sticks to motorcycle axes before any screen-to-world rotation or firing gesture. Use the authoritative control mode reported by the tick. On a mode change, clear held touches, key/gesture latches, pending actions, and stale look deltas in the client and host. A released lean touch never fires a weapon. Process a queued Interact at most once, even when a display frame produces several simulation ticks. Core mode checks remain the fallback for late frames. Pause, background, pointer cancellation, and session changes clear input using the existing reset paths.
- **Riding HUD.** Show speed and bike condition while mounted. Use the grip warning for approaching traction loss. Keep the condition indicator separate from survivor health so the player can read the consequences of a crash.
- **Android wire format.** Keep the C v2 structs and entry points compatible. Add `zb_action_v3_update` and `zb_action_v3_frame_surface` with corresponding `ZbActionFrameInputV3` and `ZbActionFrameOutputV3` structs. Compose the v2 fields with a control-kind tag, `steer/throttle/brake/lean` axes, and mounted camera preference on input, and a quantized mounted HUD on output. Keep old clients on the on-foot path. Mirror the v3 structs in `zombiebox.h`, update the JNI frame call and packed output, and include the vehicle fields in Kotlin's HUD-change comparison and React Native event. Feed engine and traction levels to local audio loops and send edge events for effects and haptics. Stop riding loops on dismount, pause, or background.
- **Browser wire format.** Extend the action request with an opt-in `frame_version=3`, the same control-kind tag and motorcycle axes, and mounted camera preference. Use `ZBF3` with the existing 64-byte header's field offsets followed by an 8-byte extension before pixels. Suggested extension fields are control kind (`u8`), grip use (`u8`), speed in tenths of mph (`u16`), engine load (`u8`), riding flags (`u8`), condition percent (`u8`), and one reserved byte. Requests without the version opt-in retain the existing `ZBF2` layout and on-foot behavior. Store the schema from the authoritative response and clear held controls on changes. Decode the payload in the same change as the encoder and update touch prompts, keyboard bindings, and vehicle audio. [Shared presentation](../crates/zombiebox-render3d/src/presentation.rs) · [Native frame ABI](../crates/zombiebox-ffi/include/zombiebox.h) · [Browser frame host](../crates/zombiebox-web/src/main.rs)

## Cruiser asset integration

The pack supplies a generated `motorcycle.rs`, five vertex-colored GLBs with rider locators, a generator, and preview images. Use the native module directly at `crates/zombiebox-render3d/src/mesh/motorcycle.rs` and run Rust formatting on it when adding it. The GLBs and generator are authoring sources. In a temporary checkout, the Rust module compiled against the current renderer and `MeshBuffer::validate()` passed for all four parts and the assembled mesh in all five paints. A separate GLB audit confirmed valid indices, finite positions, identical geometry across paints, and locator offsets and parents that match the Rust API. In-game rider fit, lighting, and handset frame pacing still need validation. [Motorcycle asset pack](zombiebox_motorcycle_pack.zip)

| Part | Mesh API enum | Rest pivot in model meters |
| --- | --- | --- |
| Chassis | `Body` | `(0, 0, 0)` at ground level between the axles |
| Fork, handlebars, front fender, and headlight | `Steering` | `(0, 0.99, 0.48)` |
| Front wheel | `FrontWheel` | `(0, 0.35, 0.835)` |
| Rear wheel | `RearWheel` | `(0, 0.345, -0.835)` |

- **Scale and axes.** The cruiser is approximately 2.49 m long, 0.99 m wide, and 1.40 m high. Start at a uniform render scale of `0.60` world units per model meter alongside the 1.05-unit survivor and tune in the chase view. Scale the part offsets and rider locators by the same factor. Keep collision dimensions and the speed-to-mph conversion in core tuning. The mesh faces local `+Z`. Use `heading_to_render_quat(heading) * Quat::from_rotation_y(FRAC_PI_2)` for upright heading, then apply local pitch and roll. Rider-right is local `-X` under that transform. Use a negative steering rotation about the pack's axis `(0, 0.874791, -0.484500)` for positive rider-right steer. The heading and steer signs passed a four-cardinal-direction transform check. [Renderer coordinates](../crates/zombiebox-render3d/src/coords.rs)
- **Articulation.** `motorcycle_part_mesh(part, paint)` returns pivot-local geometry and `motorcycle_part_pivot(part)` gives the rest offset. With vehicle root transform `M`, steering pivot `S`, axle pivots `F` and `R`, steer rotation `A`, and local X-axis wheel spin `W`, use `M` for the body, `M * T(S) * A` for the steering assembly, `M * T(S) * A * T(F-S) * W_front` for the front wheel, and `M * T(R) * W_rear` for the rear. The GLB nodes are siblings, so the renderer supplies this hierarchy. Advance wheel spin from travel distance and scaled tire radius. The assembled `motorcycle_mesh(paint)` helper is handy for rest-pose validation.
- **Rider attachment.** Call `motorcycle_locator(MotorcycleLocator) -> (MotorcyclePart, Vec3)` for the seat, left and right grips, and left and right footboards. Transform each parent-relative offset with the corresponding rendered part transform. The grips use the steering assembly, and the seat and footboards use the chassis. Use the resulting positions as targets for the rider's hips, hands, and feet. All five GLBs include the same named meshless child nodes. Check reach and clearance at full steering lock and lean in the mounted view.
- **Mobile cost.** The pack has 14,880 triangles per bike and one detail level. The current native builder emits separate vertices per face, giving about 2.2 MiB of vertex and index data per uploaded paint. Start with `GreenBlack` and upload each part once. Share the parts across bike instances and cull them before submission. Measure a single riding bike and a view with several parked bikes on the target handset. If profiling identifies the mesh as a bottleneck, use a simpler distant mesh that retains the tank, fenders, and wheels while reducing spokes, fins, and other small detail.

## Implementation slices

Complete the slices in order. Each row gives the files and an observable stopping point for the implementing model. New paths are marked with **new**.

| Slice | Files and changes | Handoff check |
| --- | --- | --- |
| 1. Generate, mount, and ride | **New** `crates/zombiebox-core/src/world/vehicles.rs`, wired from `world.rs`, and **new** `crates/zombiebox-core/src/action/vehicles/{mod,motorcycle}.rs`. Wire `action/{mod,types,county}.rs` for the registry, prompts, tick routing, and read-only views. Re-export public types from `crates/zombiebox-core/src/lib.rs` and update full `ActionInput` literals. | A repeat seed gives the same roster. A headless run can mount the starter bike, coast, brake, dismount, and remount the same ID. On-foot tests pass. |
| 2. Grip, contact, and wipeout | Extend `action/vehicles/motorcycle.rs` and `action/{mod,types,block}.rs`. Reuse `spatial/{kinematics,collision,action_adapter}.rs`, `action/ai.rs`, and the existing tile damage path. **New** `crates/zombiebox-core/tests/motorcycle.rs` for the headless cases. | Stable coordinated turn, low-speed tip, high-speed ejection and injury, riderless skid, cumulative damage, and a disabled bike's seam revisit pass headlessly. |
| 3. Render and follow | Add the supplied `mesh/motorcycle.rs` and **new** `mesh/vehicles.rs` for part registration and transforms. Register both in `mesh/mod.rs`. Wire `crates/zombiebox-render3d/src/{camera,presentation,animation,effects}.rs` and `scene/{mod,snapshot}.rs`. | The bike faces its direction of travel. The fork, front wheel, and hand targets steer together. The wheels spin around their axles and the rider follows the seat and footboards through lean. Check skid, disabled state, and camera transitions. |
| 4. Android play | `crates/zombiebox-ffi/{src/action.rs,src/action3d.rs,include/zombiebox.h,tests/host.c}`. Then `apps/mobile/android/zombiebox_jni.c`, `NativeGame.java`, `ZombieboxSurfaceView.kt`, `AudioHapticsEngine.kt`, `RetroSynth.kt`, `apps/mobile/specs/ZombieboxSurfaceNativeComponent.ts`, and `apps/mobile/App.tsx`. | A handset run uses both sticks simultaneously and clears held input and riding audio across all exit paths. |
| 5. Browser play | `crates/zombiebox-web/src/main.rs`, `apps/web/{action.js,index.html,style.css}`. Wire the keyboard and touch mappings, camera comparison, versioned frame, HUD, and browser sound. | The legacy frame contract still passes. The browser action build completes the same county ride and return. |
| 6. Tune and record | Adjust motorcycle tuning and feedback in the files above. Record touchscreen coverage in `apps/mobile/README.md` and the motorcycle milestone in `docs/product-decisions.md`. | Record the midrange Samsung model and route. Mark completion after the applicable checklist cases pass. |

## Completion checks

- **Seeded roster.** Compare same-seed placements and IDs in different chunk visit orders. Use seed `42` plus a batch of other seeds to check a short walk and clear approach to one usable starter bike, additional bikes on distinct road approaches, and variation across seeds. Teleport directly to a distant placement to verify clearance. Start a new run with a previously played seed and check that all bikes are back at their baseline positions and full condition.
- **Headless core.** Verify a coordinated road turn, a ~40 mph sharp-steer ejection with rider injury and forward riderless skid, and a ~2 mph tip that leaves a healthy bike rightable. Mount a rolling or skidding bike and confirm its momentum carries into the ride; dismount at speed, confirm the bike continues rolling, and remount it while it moves. Hold brake at a stop and verify a walking-pace reverse with no engine load, then release and confirm it settles. Exercise braking, variations in traction, non-finite input, an in-path infected, a destructible prop, a thin obstacle, a bridge deck, and deep water. Verify that a wall blocks impacts beyond it and a resting contact costs condition once. Accumulate damage to zero, then verify that the disabled bike can finish its skid and stays inoperable after it stops.
- **Run continuity.** Use seed `42` to ride several chunks from the residential start into another district, cross a bridge, park just over a seam, leave streaming range, and return. Repeat after a wipeout that disables the bike. Check for exactly one instance of the moved ID at its final pose and condition, with its baseline spawn vacant and other bikes unaffected. Exercise pause/resume, same-run atlas travel on foot, and rejected mounted teleport.
- **Controls and presentation.** Compare mounted chase and fixed isometric while turning in both directions. Check model scale, rider fit, steering direction and wheel clearance at full lock, tire contact while leaning, and a readable silhouette at dusk. Mount, dismount, and crash while sticks are held. Check moving mounts, moving dismounts, rolling remounts, reverse pushback, throttle, brake, lean, weapon gating, safe dismount prompts, and steering/lean feedback on Android touch and browser touch/keyboard. Verify Birch Street movement, fighting, room cutaways, and camera switching on foot.
- **Build and performance.** Run `cargo test --locked --workspace`, `cargo fmt --all -- --check`, and `sh scripts/check-native.sh`. Add protocol coverage for legacy and vehicle frames. Build the APK and record host, emulator, and physical handset runs separately. On a midrange Samsung Android handset, complete the route using the touchscreen. Record model, seed and route, p95 frame time against the 16.7 ms target, and any hitches at chunk or district seams, fast turns, and wipeouts. Compare frame time and submitted triangles with one bike and with several visible bikes. [Product completion checks](product-decisions.md#planned-milestones-and-completion-checks) · [Device criteria](architecture.md#first-device-decision)
