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
use crate::mesh::motorcycle::{MotorcyclePart, motorcycle_part_mesh};
use crate::mesh::props::{build_chunk_props_mesh, cache_instances, cache_mesh};
use crate::mesh::vehicles::{MotorcycleTransforms, default_motorcycle_paint};
use crate::telemetry::{FramePercentiles, FrameTelemetry, FrameTiming};
use crate::{
    CameraFrame, CameraMode, CameraRig, CameraTarget, ChunkCoord, FrameSnapshot, RgbaImage,
    SceneConfig, SceneManager, build_chunk_terrain_mesh, weapon_look_ahead_distance,
};
use std::collections::BTreeMap;
use zombiebox_core::{MotorcyclePhase, VehicleView};

/// Shared camera, scene, and rendering state for a single action run.
pub struct Presentation {
    camera: CameraRig,
    scene: SceneManager,
    renderer: GpuRenderer,
    actors: ActorAnimator,
    actor_prototypes: [PrototypeMeshId; ActorPart::MESH_COUNT],
    cache_prototypes: [PrototypeMeshId; 2],
    motorcycle_prototypes: [PrototypeMeshId; 4],
    door_prototype: PrototypeMeshId,
    doors: BTreeMap<ChunkCoord, Vec<AnimatedDoor>>,
    motorcycle_wheel_phase: BTreeMap<zombiebox_core::VehicleId, f32>,
    mounted_camera_preference_isometric: bool,
    effects: Effects,
    telemetry: FrameTelemetry,
    adaptive_resolution: bool,
}

const CACHE_BATCH_COUNT: usize = 2;
const MOTORCYCLE_BATCH_COUNT: usize = 4;
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
        let motorcycle_prototypes = [
            MotorcyclePart::Body,
            MotorcyclePart::Steering,
            MotorcyclePart::FrontWheel,
            MotorcyclePart::RearWheel,
        ]
        .map(|part| {
            let mesh = motorcycle_part_mesh(part, default_motorcycle_paint());
            renderer.register_prototype(
                ctx,
                match part {
                    MotorcyclePart::Body => "motorcycle_body",
                    MotorcyclePart::Steering => "motorcycle_steering",
                    MotorcyclePart::FrontWheel => "motorcycle_front_wheel",
                    MotorcyclePart::RearWheel => "motorcycle_rear_wheel",
                },
                &mesh,
            )
        });
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
        let body = game.player_body();
        let pose = game.player_pose();
        let mounted_id = game.mounted_vehicle();
        let mounted_view = mounted_id.and_then(|id| {
            game.vehicle_views()
                .find_map(|(candidate, view)| (candidate == id).then_some(view))
        });
        let target = CameraTarget {
            position: mounted_view.map_or(body.position, |v| match v {
                VehicleView::Motorcycle(m) => m.body.position,
            }),
            velocity: mounted_view.map_or(body.velocity, |v| match v {
                VehicleView::Motorcycle(m) => m.body.velocity,
            }),
            aim: pose.facing,
            heading: mounted_view.map_or(body.heading, |v| match v {
                VehicleView::Motorcycle(m) => m.body.heading,
            }),
            look_ahead_distance: if mounted_id.is_some() {
                0.0
            } else {
                weapon_look_ahead_distance(game.inventory().equipped)
            },
        };
        self.camera.set_obstruction_distance(None);
        let mut camera = self.camera.update(target, dt);
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
            let forward = moto.body.facing_xy();
            let signed_speed = moto.body.velocity.xy().dot(forward);
            let wheel_direction = if signed_speed < -0.05 { -1.0 } else { 1.0 };
            let speed = moto.speed.abs() * wheel_direction;
            let phase = self.motorcycle_wheel_phase.entry(*id).or_default();
            if dt.is_finite() {
                *phase += speed * dt.clamp(0.0, 0.1) / (0.21 * 2.0);
            }
            let transforms = MotorcycleTransforms::from_view(moto, *phase);
            let parts = [
                MotorcyclePart::Body,
                MotorcyclePart::Steering,
                MotorcyclePart::FrontWheel,
                MotorcyclePart::RearWheel,
            ];
            for (index, part) in parts.into_iter().enumerate() {
                let mut instance = InstanceRaw::from_transform(transforms.part_transform(part));
                if moto.phase == MotorcyclePhase::Tipped || moto.condition == 0 {
                    instance = instance.with_tint([0.62, 0.62, 0.62, 1.0]);
                }
                instances[vehicle_base + index].push(instance);
            }
        }
        (camera, frame, lighting, instances)
    }

    fn batches<'a>(
        actor_prototypes: [PrototypeMeshId; ActorPart::MESH_COUNT],
        cache_prototypes: [PrototypeMeshId; 2],
        motorcycle_prototypes: [PrototypeMeshId; 4],
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
        for (index, mesh) in motorcycle_prototypes.into_iter().enumerate() {
            batches.push(InstanceBatch {
                mesh,
                instances: &instances[ActorPart::MESH_COUNT + CACHE_BATCH_COUNT + index],
            });
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
            self.motorcycle_prototypes,
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
            self.motorcycle_prototypes,
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
}
