use glam::{Mat4, UVec2, Vec3};
use std::time::Instant;
use zombiebox_core::{ActionEvent, ActionGame, FLOOR_H, SpatialWorld, TileKind};

use crate::animation::ActorAnimator;
use crate::coords::render_to_sim_point;
use crate::effects::Effects;
use crate::gpu::{
    FrameStats, GpuContext, GpuError, GpuRenderer, InstanceBatch, InstanceRaw, PrototypeMeshId,
    RenderPipelineConfig, SceneLightingUniforms, SurfaceFrameOutcome,
};
use crate::mesh::actors::{ActorMeshLibrary, ActorPart};
use crate::mesh::buildings::build_chunk_building_shell;
use crate::mesh::doors::{AnimatedDoor, door_leaf_mesh, door_leaves};
use crate::mesh::motorcycle::{
    MotorcycleLocator, MotorcyclePart, motorcycle_locator, motorcycle_part_mesh_with_appearance,
};
use crate::mesh::motorcycle_appearance::{
    MOTORCYCLE_SLOT_COUNT, MotorcycleVisualKey, motorcycle_slot, visual_key,
};
use crate::mesh::props::{build_chunk_props_mesh, cache_instances, cache_mesh};
use crate::mesh::vehicles::{MOTORCYCLE_SCALE, MotorcycleTransforms};
use crate::telemetry::{FramePercentiles, FrameTelemetry, FrameTiming};
use crate::{
    CameraFrame, CameraMode, CameraRig, CameraTarget, ChunkCoord, FrameSnapshot, RgbaImage,
    SceneConfig, SceneManager, build_chunk_terrain_mesh, weapon_look_ahead_distance,
};
use std::collections::BTreeMap;
use zombiebox_core::VehicleView;

/// Shared camera, scene, and rendering state for a single action run.
pub struct Presentation {
    camera: CameraRig,
    scene: SceneManager,
    renderer: GpuRenderer,
    actors: ActorAnimator,
    actor_prototypes: [PrototypeMeshId; ActorPart::MESH_COUNT],
    cache_prototypes: [PrototypeMeshId; 2],
    motorcycle_prototypes: [Option<MotorcyclePrototypeSet>; MOTORCYCLE_SLOT_COUNT],
    door_prototype: PrototypeMeshId,
    doors: BTreeMap<ChunkCoord, Vec<AnimatedDoor>>,
    motorcycle_wheel_phase: BTreeMap<zombiebox_core::VehicleId, f32>,
    mounted_camera_preference_isometric: bool,
    effects: Effects,
    telemetry: FrameTelemetry,
    adaptive_resolution: bool,
}

const MOTORCYCLE_PARTS: [MotorcyclePart; 4] = [
    MotorcyclePart::Body,
    MotorcyclePart::Steering,
    MotorcyclePart::FrontWheel,
    MotorcyclePart::RearWheel,
];

/// Four GPU prototype IDs reused for one live motorcycle slot.
#[derive(Clone, Copy)]
struct MotorcyclePrototypeSet {
    key: MotorcycleVisualKey,
    ids: [PrototypeMeshId; 4],
}

const CACHE_BATCH_COUNT: usize = 2;
const MOTORCYCLE_BATCH_COUNT: usize = MOTORCYCLE_SLOT_COUNT * MOTORCYCLE_PARTS.len();
const DOOR_BATCH: usize = ActorPart::MESH_COUNT + CACHE_BATCH_COUNT + MOTORCYCLE_BATCH_COUNT;
const BATCH_COUNT: usize = DOOR_BATCH + 1;

impl Presentation {
    pub fn new(ctx: &GpuContext, width: u32, height: u32) -> Self {
        let mut renderer = GpuRenderer::new(ctx, RenderPipelineConfig::default());
        let actors = ActorMeshLibrary::new();
        let actor_prototypes = std::array::from_fn(|index| {
            let part = ActorPart::MESH_PARTS[index];
            let label = format!("actor_mesh_{index}");
            renderer.register_prototype(ctx, &label, actors.mesh(part))
        });
        let cache_prototypes = [
            renderer.register_prototype(ctx, "closed_cache", cache_mesh(false)),
            renderer.register_prototype(ctx, "open_cache", cache_mesh(true)),
        ];
        let door_prototype = renderer.register_prototype(ctx, "door_leaf", &door_leaf_mesh());
        let motorcycle_prototypes = [None; MOTORCYCLE_SLOT_COUNT];
        Self {
            camera: CameraRig::new(width, height),
            scene: SceneManager::with_config(SceneConfig {
                active_chunk_radius: 2,
                clamp_to_world_bounds: true,
                sync_mode: crate::CacheSyncMode::EventDriven,
                ..SceneConfig::default()
            }),
            renderer,
            actors: ActorAnimator::new(),
            actor_prototypes,
            cache_prototypes,
            motorcycle_prototypes,
            door_prototype,
            doors: BTreeMap::new(),
            motorcycle_wheel_phase: BTreeMap::new(),
            mounted_camera_preference_isometric: false,
            effects: Effects::new(),
            telemetry: FrameTelemetry::default(),
            adaptive_resolution: false,
        }
    }

    pub fn camera(&self) -> &CameraRig {
        &self.camera
    }

    pub fn camera_mut(&mut self) -> &mut CameraRig {
        &mut self.camera
    }

    /// Select the fixed isometric mounted view (`true`) or heading-following chase view (`false`).
    pub fn set_mounted_camera_preference(&mut self, isometric: bool) {
        self.mounted_camera_preference_isometric = isometric;
    }

    pub fn observe_tick(&mut self, game: &ActionGame) {
        self.scene.observe_tick(game);
    }

    pub fn reset(&mut self) {
        self.motorcycle_wheel_phase.clear();
        self.doors.clear();
        self.scene.reset();
        self.renderer.chunks_mut().clear();
        self.actors = ActorAnimator::new();
        self.effects = Effects::new();
        self.telemetry = FrameTelemetry::default();
        self.camera.notify_teleport();
        // Prototype slots are intentionally retained: the renderer's prototype
        // Vec never shrinks, so re-registering would leak slots across runs.
    }

    /// Uploads (or refreshes in place) the four part meshes for one motorcycle slot.
    fn ensure_motorcycle_prototypes(
        &mut self,
        ctx: &GpuContext,
        slot: usize,
        key: MotorcycleVisualKey,
    ) {
        let Some(existing) = self.motorcycle_prototypes.get(slot).copied() else {
            return;
        };
        if let Some(set) = existing {
            if set.key == key {
                return;
            }
        }

        let meshes = MOTORCYCLE_PARTS.map(|part| motorcycle_part_mesh_with_appearance(part, key));
        let ids = match existing {
            Some(set) => {
                for (index, part) in MOTORCYCLE_PARTS.into_iter().enumerate() {
                    let label = format!("motorcycle_{slot}_{part:?}");
                    self.renderer
                        .update_prototype(ctx, set.ids[index], &label, &meshes[index]);
                }
                set.ids
            }
            None => std::array::from_fn(|index| {
                let label = format!("motorcycle_{slot}_{:?}", MOTORCYCLE_PARTS[index]);
                self.renderer
                    .register_prototype(ctx, &label, &meshes[index])
            }),
        };
        self.motorcycle_prototypes[slot] = Some(MotorcyclePrototypeSet { key, ids });
    }

    pub fn notify_teleport(&mut self) {
        self.scene.notify_teleport();
        self.actors.notify_teleport();
        self.effects = Effects::new();
        self.camera.notify_teleport();
    }

    pub fn last_frame_stats(&self) -> FrameStats {
        self.renderer.last_frame_stats()
    }

    pub fn set_adaptive_resolution(&mut self, enabled: bool) {
        if self.adaptive_resolution != enabled {
            self.telemetry = FrameTelemetry::default();
            self.adaptive_resolution = enabled;
        }
    }

    pub fn latest_frame_timing(&self) -> Option<FrameTiming> {
        self.telemetry.latest()
    }

    pub fn frame_percentiles(&self) -> FramePercentiles {
        self.telemetry.percentiles(120)
    }

    /// Exclude a known suspend/reset gap from the next frame timing interval.
    pub fn interrupt_frame_timing(&mut self) {
        self.telemetry.interrupt();
    }

    fn update_camera(camera: &mut CameraRig, game: &ActionGame, dt: f32) -> CameraFrame {
        let mounted_id = game.mounted_vehicle();
        let mounted_view = mounted_id.and_then(|id| {
            game.vehicle_views()
                .find_map(|(candidate, view)| (candidate == id).then_some(view))
        });
        let body = match mounted_view {
            Some(VehicleView::Motorcycle(m)) => m.body,
            None => game.player_body(),
        };
        let mut position = body.position;
        if let (Some(VehicleView::Motorcycle(_)), CameraMode::FirstPerson) =
            (mounted_view, camera.mode())
        {
            let (_, seat) = motorcycle_locator(MotorcycleLocator::Seat);
            let (sin, cos) = body.heading.sin_cos();
            let seat_offset = seat.z * MOTORCYCLE_SCALE;
            position.x += cos * seat_offset;
            position.y += sin * seat_offset;
            // Give the seated view clearance over the handlebars and mirrors.
            position.z += 0.10;
        }
        let target = CameraTarget {
            position,
            velocity: body.velocity,
            aim: game.player_pose().facing,
            heading: body.heading,
            look_ahead_distance: if mounted_id.is_some() {
                0.0
            } else {
                weapon_look_ahead_distance(game.inventory().equipped)
            },
        };
        camera.set_obstruction_distance(None);
        if mounted_view.is_some() {
            camera.update_mounted(target, dt)
        } else {
            camera.update(target, dt)
        }
    }

    fn prepare(
        &mut self,
        ctx: &GpuContext,
        game: &ActionGame,
        dt: f32,
    ) -> (
        CameraFrame,
        FrameSnapshot,
        SceneLightingUniforms,
        [Vec<InstanceRaw>; BATCH_COUNT],
    ) {
        let mut camera = Self::update_camera(&mut self.camera, game, dt);
        if matches!(camera.mode, CameraMode::Orbit | CameraMode::Chase) {
            let start = render_to_sim_point(camera.focus);
            let end = render_to_sim_point(camera.eye);
            if let Some(hit) = game.spatial().sphere_cast(start, end, 0.08) {
                self.camera
                    .set_obstruction_distance(Some((hit.distance - 0.04).max(0.0)));
                camera = self.camera.current_frame();
            }
        }
        let frame = self.scene.prepare(game, &camera, dt);
        let is_active = |coord: ChunkCoord| {
            frame
                .active_chunks
                .binary_search_by_key(&(coord.cy, coord.cx), |c| (c.cy, c.cx))
                .is_ok()
        };
        self.renderer
            .chunks_mut()
            .retain_chunks(|coord, _| is_active(coord));
        self.doors.retain(|coord, _| is_active(*coord));
        for delta in &frame.dirty_chunks {
            let doors = self.doors.entry(delta.coord).or_default();
            let previous = std::mem::take(doors);
            doors.extend(door_leaves(&delta.snapshot).map(|leaf| {
                let matching = previous.iter().find(|door| door.leaf.tile == leaf.tile);
                let toggled = frame.events.iter().any(
                    |event| matches!(event, ActionEvent::DoorToggled { at, .. } if *at == leaf.tile),
                );
                AnimatedDoor::new(leaf, matching, toggled)
            }));
            let mut mesh = build_chunk_terrain_mesh(&delta.snapshot);
            mesh.append(&build_chunk_building_shell(&delta.snapshot));
            mesh.append(&build_chunk_props_mesh(&delta.snapshot));
            let bounds = mesh
                .compute_aabb()
                .unwrap_or_else(|| delta.snapshot.bounds_render_aabb());
            self.renderer
                .upload_chunk(ctx, delta.coord, delta.revision, bounds, &mesh);
        }
        self.renderer.update_visibility(ctx, &frame.dynamic);
        let lighting = SceneLightingUniforms::for_scene(&frame.dynamic, &camera);
        self.actors.set_player_camera(camera);
        self.actors
            .observe_events_with_ground(&frame.events, |xy, floor| {
                game.ground_normal_at(xy, floor)
            });
        let mut instances: [Vec<InstanceRaw>; BATCH_COUNT] = std::array::from_fn(|_| Vec::new());
        let actors = self.actors.prepare_with_openings(
            &frame.dynamic,
            dt,
            |tile, player: Vec3| {
                self.doors
                    .get(&ChunkCoord::from_pos(tile.pos()))?
                    .iter()
                    .find(|door| door.leaf.tile == tile)
                    .map(|door| door.leaf.handle(door.openness(), player))
            },
            |tile| {
                let info = game.tile_render_info(tile.pos(), tile.floor)?;
                if info.kind != TileKind::BrokenWindow {
                    return None;
                }
                let building = game.buildings().iter().find(|b| b.id == info.building_id)?;
                Some((
                    info.orientation,
                    building.foundation_z + f32::from(tile.floor) * FLOOR_H,
                ))
            },
        );
        for (part, part_instances) in actors {
            instances[part.mesh_index()].extend(part_instances.into_iter().map(|actor| {
                let mut instance =
                    InstanceRaw::from_transform(Mat4::from_cols_array_2d(&actor.model))
                        .with_tint(actor.color_tint)
                        .with_hit_flash_and_emissive(
                            (actor.emissive_flash[0] * 255.0) as u8,
                            (actor.emissive_flash[1] * 255.0) as u8,
                        );
                instance.instance_meta[..2].copy_from_slice(&actor.meta_override);
                instance
            }));
        }
        for cache in cache_instances(&frame.dynamic.caches) {
            instances[ActorPart::MESH_COUNT + usize::from(cache.opened)]
                .push(InstanceRaw::from_transform(cache.transform).with_tint(cache.tint));
        }
        let door_dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        for door in self.doors.values_mut().flatten() {
            instances[DOOR_BATCH].push(
                InstanceRaw::from_transform(door.leaf.model(door.openness()))
                    .with_tint(door.leaf.tint())
                    .with_meta_override(door.leaf.meta),
            );
            door.advance(door_dt);
        }
        self.motorcycle_wheel_phase.retain(|id, _| {
            frame
                .dynamic
                .vehicles
                .iter()
                .any(|(vehicle_id, _)| vehicle_id == id)
        });
        let vehicle_base = ActorPart::MESH_COUNT + CACHE_BATCH_COUNT;
        for (id, view) in &frame.dynamic.vehicles {
            let VehicleView::Motorcycle(moto) = *view;
            let Some(slot) = motorcycle_slot(*id) else {
                debug_assert!(false, "unexpected motorcycle slot for vehicle {id:?}");
                continue;
            };
            self.ensure_motorcycle_prototypes(ctx, slot, visual_key(*id, moto.condition));
            let forward = moto.body.facing_xy();
            let signed_speed = moto.body.velocity.xy().dot(forward);
            let wheel_direction = if signed_speed < -0.05 { -1.0 } else { 1.0 };
            let speed = moto.speed.abs() * wheel_direction;
            let phase = self.motorcycle_wheel_phase.entry(*id).or_default();
            if dt.is_finite() {
                *phase += speed * dt.clamp(0.0, 0.1) / (0.21 * 2.0);
            }
            let transforms = MotorcycleTransforms::from_view(moto, *phase);
            let slot_base = vehicle_base + slot * MOTORCYCLE_PARTS.len();
            for (index, part) in MOTORCYCLE_PARTS.into_iter().enumerate() {
                // Wear (and not pose) carries damage into the baked vertex colors.
                instances[slot_base + index]
                    .push(InstanceRaw::from_transform(transforms.part_transform(part)));
            }
        }
        (camera, frame, lighting, instances)
    }

    fn batches<'a>(
        actor_prototypes: [PrototypeMeshId; ActorPart::MESH_COUNT],
        cache_prototypes: [PrototypeMeshId; 2],
        motorcycle_prototypes: &[Option<MotorcyclePrototypeSet>; MOTORCYCLE_SLOT_COUNT],
        door_prototype: PrototypeMeshId,
        instances: &'a [Vec<InstanceRaw>; BATCH_COUNT],
    ) -> Vec<InstanceBatch<'a>> {
        let mut batches = Vec::with_capacity(BATCH_COUNT);
        for (index, mesh) in actor_prototypes.into_iter().enumerate() {
            batches.push(InstanceBatch {
                mesh,
                instances: &instances[index],
            });
        }
        for (index, mesh) in cache_prototypes.into_iter().enumerate() {
            batches.push(InstanceBatch {
                mesh,
                instances: &instances[ActorPart::MESH_COUNT + index],
            });
        }
        for (slot, set) in motorcycle_prototypes.iter().enumerate() {
            let Some(set) = set else {
                continue;
            };
            for (index, mesh) in set.ids.into_iter().enumerate() {
                let batch = ActorPart::MESH_COUNT
                    + CACHE_BATCH_COUNT
                    + slot * MOTORCYCLE_PARTS.len()
                    + index;
                if instances[batch].is_empty() {
                    continue;
                }
                batches.push(InstanceBatch {
                    mesh,
                    instances: &instances[batch],
                });
            }
        }
        batches.push(InstanceBatch {
            mesh: door_prototype,
            instances: &instances[DOOR_BATCH],
        });
        batches
    }

    pub fn render_offscreen(
        &mut self,
        ctx: &GpuContext,
        game: &ActionGame,
        dt: f32,
    ) -> Result<RgbaImage, GpuError> {
        let started = Instant::now();
        let (camera, frame, lighting, instances) = self.prepare(ctx, game, dt);
        let effects = self.effects.prepare(&frame, dt);
        let batches = Self::batches(
            self.actor_prototypes,
            self.cache_prototypes,
            &self.motorcycle_prototypes,
            self.door_prototype,
            &instances,
        );
        let prepare_time = started.elapsed();
        let render_started = Instant::now();
        let rendered =
            self.renderer
                .render_offscreen_rgba8(ctx, &camera, &lighting, &batches, Some(&effects));
        if rendered.is_ok() {
            self.telemetry.record(
                started,
                prepare_time,
                render_started.elapsed(),
                self.last_frame_stats(),
                false,
            );
        }
        rendered
    }

    pub fn render_to_surface(
        &mut self,
        ctx: &GpuContext,
        surface: &wgpu::Surface<'_>,
        config: &wgpu::SurfaceConfiguration,
        game: &ActionGame,
        dt: f32,
    ) -> Result<SurfaceFrameOutcome, GpuError> {
        let started = Instant::now();
        self.camera.set_display_surface(config.width, config.height);
        let reference_size = self.camera.internal_size();
        let scale = if self.adaptive_resolution {
            self.telemetry.resolution_percent()
        } else {
            100
        };
        let original_iso_scale = self.camera.config().iso_pixels_per_unit;
        if scale < 100 {
            self.camera.set_viewport_and_internal_size(
                self.camera.viewport_size(),
                (reference_size * scale / 100).max(UVec2::ONE),
            );
            let ratio = self.camera.internal_size().y as f32 / reference_size.y as f32;
            self.camera.config_mut().iso_pixels_per_unit *= ratio;
        }
        let (camera, frame, lighting, instances) = self.prepare(ctx, game, dt);
        self.camera.config_mut().iso_pixels_per_unit = original_iso_scale;
        self.camera.set_display_surface(config.width, config.height);
        let effects = self.effects.prepare(&frame, dt);
        let batches = Self::batches(
            self.actor_prototypes,
            self.cache_prototypes,
            &self.motorcycle_prototypes,
            self.door_prototype,
            &instances,
        );
        let prepare_time = started.elapsed();
        let render_started = Instant::now();
        let rendered = self.renderer.render_to_surface(
            ctx,
            surface,
            config,
            &camera,
            &lighting,
            &batches,
            Some(&effects),
        );
        if let Ok(SurfaceFrameOutcome::Presented { stats, .. }) = rendered {
            self.telemetry.record(
                started,
                prepare_time,
                render_started.elapsed(),
                stats,
                self.adaptive_resolution,
            );
        }
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zombiebox_core::{ActionGame, ActionInput, Tile3D};

    fn make_test_game() -> ActionGame {
        let mut game = ActionGame::county(42);
        let (_, bike_pos) = game
            .vehicle_views()
            .find_map(|(id, view)| {
                (id.0 & 0xff == 0).then(|| match view {
                    VehicleView::Motorcycle(bike) => (id, bike.body.position.xy()),
                })
            })
            .expect("seed 42 starter motorcycle");
        assert!(game.teleport(Tile3D::from_vec2(bike_pos, 0)).is_some());
        game
    }

    #[test]
    fn seed_42_mount_aligns_view_and_dismount_keeps_facing() {
        let mut game = make_test_game();
        let mut camera = CameraRig::new(320, 180);
        camera.set_mode(CameraMode::Chase);
        Presentation::update_camera(&mut camera, &game, 0.016);
        camera.adjust_first_person_look(1.0, 0.0);
        let before = Presentation::update_camera(&mut camera, &game, 0.016);

        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        let mounted_id = game.mounted_vehicle().expect("mounted starter bike");
        let heading = game
            .vehicle_views()
            .find_map(|(id, view)| {
                (id == mounted_id).then_some(match view {
                    VehicleView::Motorcycle(bike) => bike.body.heading,
                })
            })
            .unwrap();
        let mounted = Presentation::update_camera(&mut camera, &game, 0.016);
        let expected = heading + std::f32::consts::PI;
        assert!(crate::coords::shortest_angle_delta(expected, before.yaw).abs() > 0.5);
        assert!(crate::coords::shortest_angle_delta(expected, mounted.yaw).abs() < 1e-4);
        assert_eq!(mounted.mode, CameraMode::Chase);

        camera.set_mode(CameraMode::FirstPerson);
        let first_person = Presentation::update_camera(&mut camera, &game, 0.016);
        assert!(crate::coords::shortest_angle_delta(expected, first_person.yaw).abs() < 1e-4);
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert!(game.mounted_vehicle().is_none());
        let dismounted = Presentation::update_camera(&mut camera, &game, 0.016);
        assert!(crate::coords::shortest_angle_delta(first_person.yaw, dismounted.yaw).abs() < 1e-4);
        camera.adjust_first_person_look(0.3, 0.0);
        let looked = Presentation::update_camera(&mut camera, &game, 0.016);
        assert!(crate::coords::shortest_angle_delta(dismounted.yaw, looked.yaw) > 0.29);
    }

    #[test]
    fn motorcycle_riding_preserves_selected_camera_modes() {
        let Ok(ctx) = GpuContext::new_headless() else {
            return;
        };

        let mut presentation = Presentation::new(&ctx, 320, 180);

        for mode in CameraMode::ALL {
            let mut game = make_test_game();
            presentation.camera_mut().set_mode(mode);
            assert_eq!(presentation.camera().mode(), mode);

            // Prepare on foot: camera mode unchanged
            let (frame, ..) = presentation.prepare(&ctx, &game, 0.016);
            assert_eq!(frame.mode, mode);
            assert_eq!(presentation.camera().mode(), mode);

            // Mount motorcycle
            game.step(ActionInput {
                interact: true,
                ..ActionInput::default()
            });
            assert!(game.mounted_vehicle().is_some());

            // Prepare while mounted: camera mode must stay the selected mode!
            let (frame, ..) = presentation.prepare(&ctx, &game, 0.016);
            assert_eq!(
                frame.mode, mode,
                "camera mode must be preserved while riding: expected {:?}, got {:?}",
                mode, frame.mode
            );
            assert_eq!(presentation.camera().mode(), mode);

            // Dismount motorcycle
            game.step(ActionInput {
                interact: true,
                ..ActionInput::default()
            });
            assert!(game.mounted_vehicle().is_none());

            // Prepare after dismount: camera mode still unchanged
            let (frame, ..) = presentation.prepare(&ctx, &game, 0.016);
            assert_eq!(frame.mode, mode);
            assert_eq!(presentation.camera().mode(), mode);
        }
    }

    #[test]
    fn batch_layout_keeps_motorcycle_slots_before_doors() {
        assert_eq!(MOTORCYCLE_BATCH_COUNT, 16);
        assert_eq!(
            DOOR_BATCH,
            ActorPart::MESH_COUNT + CACHE_BATCH_COUNT + MOTORCYCLE_BATCH_COUNT
        );
        for slot in 0..MOTORCYCLE_SLOT_COUNT {
            for part in 0..MOTORCYCLE_PARTS.len() {
                let index = ActorPart::MESH_COUNT + CACHE_BATCH_COUNT + slot * 4 + part;
                assert!(
                    index < DOOR_BATCH,
                    "vehicle batch {index} overlaps door batch"
                );
            }
        }
    }

    #[test]
    fn motorcycle_prototype_cache_is_bounded_reused_and_retained() {
        let Ok(ctx) = GpuContext::new_headless() else {
            return;
        };
        let mut presentation = Presentation::new(&ctx, 320, 180);
        let base = presentation.renderer.prototype_count();
        assert!(
            presentation
                .motorcycle_prototypes
                .iter()
                .all(Option::is_none)
        );

        let id0 = zombiebox_core::VehicleId(0xABCD_EF00);
        let pristine = visual_key(id0, 100);
        presentation.ensure_motorcycle_prototypes(&ctx, 0, pristine);
        let set0 = presentation.motorcycle_prototypes[0].expect("slot 0 uploaded");
        assert_eq!(set0.key, pristine);
        assert_eq!(presentation.renderer.prototype_count(), base + 4);

        // An unchanged key must reuse the same four IDs.
        presentation.ensure_motorcycle_prototypes(&ctx, 0, pristine);
        assert_eq!(presentation.renderer.prototype_count(), base + 4);
        assert_eq!(presentation.motorcycle_prototypes[0].unwrap().ids, set0.ids);

        // A new damage bucket updates the same IDs in place.
        let damaged = visual_key(id0, 19);
        assert_ne!(damaged, pristine);
        presentation.ensure_motorcycle_prototypes(&ctx, 0, damaged);
        assert_eq!(presentation.renderer.prototype_count(), base + 4);
        assert_eq!(presentation.motorcycle_prototypes[0].unwrap().ids, set0.ids);
        assert_eq!(presentation.motorcycle_prototypes[0].unwrap().key, damaged);

        // Every other slot gets its own distinct set.
        let mut ids = std::collections::HashSet::new();
        ids.extend(set0.ids);
        for slot in 1..MOTORCYCLE_SLOT_COUNT {
            let key = visual_key(zombiebox_core::VehicleId(0xABCD_EF00 | slot as u64), 100);
            presentation.ensure_motorcycle_prototypes(&ctx, slot, key);
            let set = presentation.motorcycle_prototypes[slot].expect("slot uploaded");
            assert_eq!(set.key, key);
            for id in set.ids {
                assert!(ids.insert(id), "prototype id reused across slots");
            }
        }
        assert_eq!(presentation.renderer.prototype_count(), base + 16);

        // Reset must not drop or reallocate prototype slots.
        presentation.reset();
        let retained: Vec<_> = presentation
            .motorcycle_prototypes
            .iter()
            .map(|set| set.expect("reset retains populated slots").key)
            .collect();
        for (slot, key) in retained.into_iter().enumerate() {
            presentation.ensure_motorcycle_prototypes(&ctx, slot, key);
        }
        assert_eq!(presentation.renderer.prototype_count(), base + 16);

        // The real render path updates an existing slot rather than growing the cache.
        let game = make_test_game();
        presentation
            .render_offscreen(&ctx, &game, 0.016)
            .expect("offscreen frame");
        let after_first = presentation.renderer.prototype_count();
        assert!(after_first <= base + 16);
        presentation
            .render_offscreen(&ctx, &game, 0.016)
            .expect("offscreen frame");
        assert_eq!(presentation.renderer.prototype_count(), after_first);
    }
}
