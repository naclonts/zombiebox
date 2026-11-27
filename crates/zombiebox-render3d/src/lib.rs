//! 3D presentation and `wgpu` rendering engine for Zombiebox.

// Link feature-gated viewer crate explicitly while viewer runtime is a T00 stub.
#[cfg(feature = "viewer")]
use winit as _;

pub mod animation;
pub mod camera;
pub mod coords;
pub mod effects;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod materials;
mod player_rig;
pub mod mesh;
#[cfg(feature = "gpu")]
mod presentation;
pub mod scene;
#[cfg(feature = "gpu")]
pub use presentation::Presentation;
#[cfg(feature = "gpu")]
mod telemetry;
#[cfg(feature = "gpu")]
pub use telemetry::{FramePercentiles, FrameTiming};

pub use camera::{
    CameraConfig, CameraFrame, CameraGpuUniforms, CameraMode, CameraRig, CameraTarget, Frustum,
    FrustumPlane, ISO_PIXELS_PER_UNIT, RenderRay, ScreenPoint, SimRay, compute_internal_resolution,
    weapon_look_ahead_distance,
};
pub use coords::{ChunkCoord, FLOOR_H, RenderAabb, RenderVec3, SimAabb3, SimVec3};
pub use mesh::{
    CardinalDir, EXTERIOR_FLOOR_SENTINEL, FaceMask, GeometryTag, MaterialFlags, MeshBuffer,
    MeshBuilder, MeshValidationError, RidgeAxis, TerrainMeshConfig, VertexMeta, WorldVertex,
    build_chunk_terrain_mesh, build_chunk_terrain_mesh_with_config,
};
pub use scene::{
    BuildingCutawayState, BuildingSnapshot, CacheSnapshot, CacheSyncMode, CachedChunk,
    ChunkSnapshot, ChunkTerrainSnapshot, DirtyChunkDelta, DynamicSceneState, FloorCellSnapshot,
    FrameSnapshot, LocalVisibilityGrid, PlayerSnapshot, SceneCache, SceneConfig, SceneManager,
    TileSnapshot, VisibilityCell, halo_affected_chunks,
};

#[cfg(feature = "gpu")]
pub use gpu::{
    ChunkGpuSlot, ChunkMeshPool, FrameStats, GpuContext, GpuError, GpuMesh, GpuRenderer,
    InstanceBatch, InstanceRaw, InternalResolutionPolicy, OffscreenTarget, PointLightGpu,
    PrototypeMeshId, RenderPipelineConfig, SceneLightingUniforms, SurfaceFrameOutcome,
    render_smoke_cube, select_surface_format, write_rgba8_png,
};

/// Applies `frame.evicted_chunks` and `frame.dirty_chunks` from `SceneManager::prepare`
/// into `GpuRenderer`'s `ChunkMeshPool`. Returns the number of chunks newly meshed & uploaded.
#[cfg(feature = "gpu")]
pub fn sync_terrain_chunks_to_gpu(
    ctx: &GpuContext,
    renderer: &mut GpuRenderer,
    frame: &FrameSnapshot,
    config: &TerrainMeshConfig,
) -> usize {
    for &coord in &frame.evicted_chunks {
        renderer.chunks_mut().remove_chunk(coord);
    }
    let mut uploaded = 0usize;
    for delta in &frame.dirty_chunks {
        if renderer.chunks().has_revision(delta.coord, delta.revision) {
            continue;
        }
        let mesh = build_chunk_terrain_mesh_with_config(&delta.snapshot, config);
        let aabb = mesh
            .compute_aabb()
            .unwrap_or_else(|| delta.snapshot.bounds_render_aabb());
        if renderer.upload_chunk(ctx, delta.coord, delta.revision, aabb, &mesh) {
            uploaded += 1;
        }
    }
    uploaded
}

/// Tightly packed row-major RGBA8888 image (`width * height * 4` bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    #[must_use]
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert_eq!(
            pixels.len(),
            (width as usize) * (height as usize) * 4,
            "RgbaImage byte length must equal width * height * 4"
        );
        Self {
            width,
            height,
            pixels,
        }
    }

    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(x < self.width && y < self.height, "pixel out of bounds");
        let idx = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ]
    }

    /// Encodes this RGBA8888 image as a PNG file at `path`.
    #[cfg(feature = "gpu")]
    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let writer = std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(writer, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut png_writer = encoder.write_header().map_err(std::io::Error::other)?;
        png_writer
            .write_image_data(&self.pixels)
            .map_err(std::io::Error::other)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_height_constant_matches_baseline() {
        assert!((FLOOR_H - 1.388).abs() < 1e-6);
    }

    #[test]
    fn rgba_image_pixel_lookup() {
        let pixels = vec![
            10, 20, 30, 255, // (0, 0)
            40, 50, 60, 255, // (1, 0)
            70, 80, 90, 255, // (0, 1)
            100, 110, 120, 255, // (1, 1)
        ];
        let img = RgbaImage::new(2, 2, pixels);
        assert_eq!(img.pixel(0, 0), [10, 20, 30, 255]);
        assert_eq!(img.pixel(1, 0), [40, 50, 60, 255]);
        assert_eq!(img.pixel(0, 1), [70, 80, 90, 255]);
        assert_eq!(img.pixel(1, 1), [100, 110, 120, 255]);
    }

    #[test]
    fn glam_mat4_bytemuck_pod_layout() {
        let mat = glam::Mat4::IDENTITY;
        let bytes = bytemuck::bytes_of(&mat);
        assert_eq!(bytes.len(), 64);
        let cols: [[f32; 4]; 4] = mat.to_cols_array_2d();
        assert_eq!(bytemuck::bytes_of(&cols), bytes);
    }
}
