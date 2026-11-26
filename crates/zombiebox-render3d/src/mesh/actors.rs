//! Reusable, device-independent actor part prototypes.
use super::{
    FaceMask, GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, emit_cuboid,
};
use glam::Vec3;

/// All parts share one unit cuboid; transforms supply their dimensions and articulation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ActorPart {
    Body,
    Head,
    Limb,
    Sac,
    Axe,
    Pistol,
    Shotgun,
    RocketLauncher,
    BoltLauncher,
}

#[derive(Clone, Debug)]
pub struct ActorMeshLibrary {
    unit: MeshBuffer,
}
impl Default for ActorMeshLibrary {
    fn default() -> Self {
        Self::new()
    }
}
impl ActorMeshLibrary {
    #[must_use]
    pub fn new() -> Self {
        let mut b = MeshBuilder::new();
        emit_cuboid(
            &mut b,
            Vec3::ZERO,
            Vec3::splat(0.5),
            FaceMask::ALL,
            [1.0; 4],
            VertexMeta::DEFAULT
                .with_tag(GeometryTag::Actor)
                .with_flags(MaterialFlags::ACTOR_MIN_LIGHT | MaterialFlags::CUTAWAY_EXEMPT),
        );
        Self { unit: b.build() }
    }
    #[must_use]
    pub fn mesh(&self, _part: ActorPart) -> &MeshBuffer {
        &self.unit
    }
}
