//! Offscreen color/depth render targets with CPU readback (`T03`).

use super::GpuError;
use crate::RgbaImage;

#[derive(Debug)]
pub struct OffscreenTarget {
    pub width: u32,
    pub height: u32,
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    pub staging_buffer: wgpu::Buffer,
}

impl OffscreenTarget {
    #[must_use]
    pub fn new(device: &wgpu::Device, width: u32, height: u32, label: &str) -> Self {
        let w = width.max(1);
        let h = height.max(1);

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("{label}_color")),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("{label}_depth")),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[wgpu::TextureFormat::Depth32Float],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded_bytes_per_row = w * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
        let staging_buffer_size = (padded_bytes_per_row * h) as wgpu::BufferAddress;

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{label}_staging")),
            size: staging_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            width: w,
            height: h,
            color_texture,
            color_view,
            depth_texture,
            depth_view,
            staging_buffer,
        }
    }

    pub fn download_rgba8(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<RgbaImage, GpuError> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("offscreen_download_encoder"),
        });

        let unpadded_bytes_per_row = self.width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(Some(encoder.finish()));

        let buffer_slice = self.staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| GpuError::ReadbackFailed)?;

        match receiver.recv() {
            Ok(Ok(())) => {
                let data = buffer_slice
                    .get_mapped_range()
                    .map_err(|_| GpuError::BufferMap)?;
                let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);
                if padded_bytes_per_row == unpadded_bytes_per_row {
                    pixels.extend_from_slice(&data);
                } else {
                    for row in 0..self.height {
                        let start = (row * padded_bytes_per_row) as usize;
                        let end = start + (self.width * 4) as usize;
                        pixels.extend_from_slice(&data[start..end]);
                    }
                }
                drop(data);
                self.staging_buffer.unmap();
                Ok(RgbaImage::new(self.width, self.height, pixels))
            }
            _ => Err(GpuError::ReadbackNotReady),
        }
    }
}
