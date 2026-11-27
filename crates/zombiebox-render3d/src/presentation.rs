use glam::{Mat4, UVec2};
use std::time::Instant;
use zombiebox_core::{ActionGame, SpatialWorld};

use crate::animation::ActorAnimator;
use crate::coords::render_to_sim_point;
use crate::effects::Effects;
use crate::gpu::{
    FrameStats, GpuContext, GpuError, GpuRenderer, InstanceBatch, InstanceRaw, PrototypeMeshId,
    RenderPipelineConfig, SceneLightingUniforms, SurfaceFrameOutcome,
};
use crate::mesh::actors::{ActorMeshLibrary, ActorPart};
use crate::mesh::buildings::build_chunk_buildings_mesh;
use crate::mesh::props::{build_chunk_props_mesh, cache_instances, cache_mesh};
use crate::telemetry::{FramePercentiles, FrameTelemetry, FrameTiming};
use crate::{
    CameraFrame, CameraMode, CameraRig, CameraTarget, FrameSnapshot, RgbaImage, SceneConfig,
    SceneManager, build_chunk_terrain_mesh, weapon_look_ahead_distance,
};

/// Shared camera, scene, and rendering state for a single action run.
pub struct Presentation {
    camera: CameraRig,
    scene: SceneManager,
    renderer: GpuRenderer,
    actors: ActorAnimator,
    actor_prototypes: [PrototypeMeshId; ActorPart::MESH_COUNT],
    cache_prototypes: [PrototypeMeshId; 2],
    effects: Effects,
    telemetry: FrameTelemetry,
    adaptive_resolution: bool,
}

const CACHE_BATCH_COUNT: usize = 2;
const BATCH_COUNT: usize = ActorPart::MESH_COUNT + CACHE_BATCH_COUNT;

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

    pub fn observe_tick(&mut self, game: &ActionGame) {
        self.scene.observe_tick(game);
    }

    pub fn reset(&mut self) {
        self.scene.reset();
        self.renderer.chunks_mut().clear();
        self.actors = ActorAnimator::new();
        self.effects = Effects::new();
        self.telemetry = FrameTelemetry::default();
        self.camera.notify_teleport();
    }

    pub fn notify_teleport(&mut self) {
        self.scene.notify_teleport();
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
        let target = CameraTarget {
            position: body.position,
            velocity: body.velocity,
            aim: pose.facing,
            heading: body.heading,
            look_ahead_distance: weapon_look_ahead_distance(game.inventory().equipped),
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
        self.renderer.chunks_mut().retain_chunks(|coord, _| {
            frame
                .active_chunks
                .binary_search_by_key(&(coord.cy, coord.cx), |c| (c.cy, c.cx))
                .is_ok()
        });
        for delta in &frame.dirty_chunks {
            let mut mesh = build_chunk_terrain_mesh(&delta.snapshot);
            mesh.append(&build_chunk_buildings_mesh(&delta.snapshot));
            mesh.append(&build_chunk_props_mesh(&delta.snapshot));
            let bounds = mesh
                .compute_aabb()
                .unwrap_or_else(|| delta.snapshot.bounds_render_aabb());
            self.renderer
                .upload_chunk(ctx, delta.coord, delta.revision, bounds, &mesh);
        }
        self.renderer.update_visibility(ctx, &frame.dynamic);
        let lighting = SceneLightingUniforms::for_scene(&frame.dynamic, &camera);
        self.actors
            .set_draw_player(camera.mode != CameraMode::FirstPerson);
        self.actors.observe_events(&frame.events);
        let mut instances: [Vec<InstanceRaw>; BATCH_COUNT] = std::array::from_fn(|_| Vec::new());
        for (part, part_instances) in self.actors.prepare(&frame.dynamic, dt) {
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
            instances[ActorPart::MESH_COUNT + usize::from(cache.opened)].push(
                InstanceRaw::from_transform(Mat4::from_translation(cache.position))
                    .with_tint(cache.tint),
            );
        }
        (camera, frame, lighting, instances)
    }

    fn batches<'a>(
        actor_prototypes: [PrototypeMeshId; ActorPart::MESH_COUNT],
        cache_prototypes: [PrototypeMeshId; 2],
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
        let batches = Self::batches(self.actor_prototypes, self.cache_prototypes, &instances);
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
        let batches = Self::batches(self.actor_prototypes, self.cache_prototypes, &instances);
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
