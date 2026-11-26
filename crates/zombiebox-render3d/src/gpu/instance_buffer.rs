//! Growable GPU instance buffer shared by every instanced draw in a frame (`T03`).

use crate::gpu::buffers::{InstanceBatch, InstanceRaw, PreparedInstanceBatch};

#[derive(Debug, Default)]
pub struct DynamicInstanceBuffer {
    buffer: Option<wgpu::Buffer>,
    capacity: usize,
    scratch: Vec<InstanceRaw>,
}

impl DynamicInstanceBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Uploads a single batch at offset zero.
    ///
    /// Only safe when this is the sole batch drawn before the next frame's
    /// upload; prefer [`Self::prepare_batches`] for multi-batch frames, since
    /// queued draw calls do not snapshot buffer contents.
    pub fn update_buffer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[InstanceRaw],
        label: &str,
    ) {
        self.scratch.clear();
        self.scratch.extend_from_slice(instances);
        if self.scratch.is_empty() {
            return;
        }
        self.upload(device, queue, label);
    }

    /// Packs every non-empty batch into one GPU buffer upload and returns the
    /// instance range each batch occupies.
    ///
    /// The returned ranges stay valid for the rest of the frame, so a draw call
    /// for one batch never observes another batch's instances.
    pub fn prepare_batches(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[InstanceBatch<'_>],
        label: &str,
    ) -> Vec<PreparedInstanceBatch> {
        self.scratch.clear();
        let mut prepared = Vec::with_capacity(batches.len());
        for batch in batches {
            if batch.instances.is_empty() {
                continue;
            }
            let start = self.scratch.len() as u32;
            self.scratch.extend_from_slice(batch.instances);
            let end = self.scratch.len() as u32;
            prepared.push(PreparedInstanceBatch {
                mesh: batch.mesh,
                instance_range: start..end,
            });
        }
        if self.scratch.is_empty() {
            return prepared;
        }
        self.upload(device, queue, label);
        prepared
    }

    /// Number of instances the GPU buffer can currently hold.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[must_use]
    pub fn buffer(&self) -> Option<&wgpu::Buffer> {
        self.buffer.as_ref()
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, label: &str) {
        let needed = self.scratch.len();
        debug_assert!(needed > 0, "upload called with an empty scratch buffer");
        if needed > self.capacity || self.buffer.is_none() {
            let new_cap = needed.max(64).next_power_of_two();
            let size = (new_cap * std::mem::size_of::<InstanceRaw>()) as wgpu::BufferAddress;
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = new_cap;
        }
        if let Some(buf) = &self.buffer {
            queue.write_buffer(buf, 0, bytemuck::cast_slice(&self.scratch));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::GpuContext;
    use crate::gpu::buffers::PrototypeMeshId;

    fn instance(x: f32) -> InstanceRaw {
        InstanceRaw::from_transform(glam::Mat4::from_translation(glam::Vec3::new(x, 0.0, 0.0)))
    }

    #[test]
    fn prepared_batches_get_disjoint_ranges_and_skip_empty_batches() {
        let Ok(ctx) = GpuContext::new_headless() else {
            eprintln!("skipping: no headless GPU adapter");
            return;
        };
        let mut buffer = DynamicInstanceBuffer::new();
        let actors = [instance(1.0), instance(2.0), instance(3.0)];
        let empty: [InstanceRaw; 0] = [];
        let closed = [instance(4.0), instance(5.0)];
        let open = [instance(6.0)];
        let batches = [
            InstanceBatch {
                mesh: PrototypeMeshId(0),
                instances: &actors,
            },
            InstanceBatch {
                mesh: PrototypeMeshId(1),
                instances: &empty,
            },
            InstanceBatch {
                mesh: PrototypeMeshId(2),
                instances: &closed,
            },
            InstanceBatch {
                mesh: PrototypeMeshId(3),
                instances: &open,
            },
        ];

        let prepared = buffer.prepare_batches(&ctx.device, &ctx.queue, &batches, "test_instances");

        assert_eq!(
            prepared,
            vec![
                PreparedInstanceBatch {
                    mesh: PrototypeMeshId(0),
                    instance_range: 0..3,
                },
                PreparedInstanceBatch {
                    mesh: PrototypeMeshId(2),
                    instance_range: 3..5,
                },
                PreparedInstanceBatch {
                    mesh: PrototypeMeshId(3),
                    instance_range: 5..6,
                },
            ]
        );
        assert!(buffer.buffer().is_some());
        assert!(buffer.capacity() >= 6);

        // A frame with no instanced geometry uploads nothing and keeps old ranges out.
        let prepared = buffer.prepare_batches(&ctx.device, &ctx.queue, &[], "test_instances");
        assert!(prepared.is_empty());
    }
}
