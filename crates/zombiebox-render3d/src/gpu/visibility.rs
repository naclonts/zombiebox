//! Per-frame visibility and cutaway data, independent of resident chunk meshes.

use crate::scene::DynamicSceneState;
use zombiebox_core::TileVisibility;

const PAGE_SIDE: u32 = 256;
pub(super) const CUTAWAY_ROWS: u32 = PAGE_SIDE;

pub(super) struct VisibilityTexture {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    cutaways: Vec<u8>,
    cells: Vec<u8>,
}

impl VisibilityTexture {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("world_visibility"),
            size: wgpu::Extent3d {
                width: PAGE_SIDE,
                height: CUTAWAY_ROWS + PAGE_SIDE * crate::scene::MAX_SNAPSHOT_FLOORS as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            cutaways: vec![255; (PAGE_SIDE * CUTAWAY_ROWS) as usize],
            cells: Vec::new(),
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, scene: &DynamicSceneState) {
        self.cutaways.fill(255);
        for building in &scene.building_cutaways {
            self.cutaways[usize::from(building.building_id)] = building.dither_opacity_u8;
        }
        self.write(queue, 0, PAGE_SIDE, CUTAWAY_ROWS, &self.cutaways);

        let grid = &scene.local_visibility;
        debug_assert!(grid.side <= PAGE_SIDE as usize);
        self.cells.clear();
        self.cells
            .extend(grid.cells.iter().map(|cell| match cell.visibility {
                TileVisibility::Hidden => 0,
                TileVisibility::Explored => 1,
                TileVisibility::Visible => 2,
            }));
        self.write(
            queue,
            CUTAWAY_ROWS,
            grid.side as u32,
            grid.side as u32 * u32::from(grid.floor_count),
            &self.cells,
        );
    }

    fn write(&self, queue: &wgpu::Queue, row: u32, width: u32, height: u32, data: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: row, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}
