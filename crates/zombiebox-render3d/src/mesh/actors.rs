//! Reusable, device-independent actor part prototypes.
use super::{
    FaceMask, GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, emit_cuboid,
    zombie::{ZombieMeshPart, zombie_mesh},
};
use glam::Vec3;

/// All parts map to one of two prototype shapes: the neutral unit cuboid used by
/// the survivor and non-shambler infected, or a pre-shaped shambler mesh part.
/// Transforms supply dimensions and articulation for both.
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
    ZombieBody,
    ZombieHead,
    ZombieArmL,
    ZombieArmR,
    ZombieLegL,
    ZombieLegR,
}

impl ActorPart {
    /// One representative part per distinct prototype mesh, in GPU batch order.
    pub const MESH_PARTS: [Self; 7] = [
        Self::Body,
        Self::ZombieBody,
        Self::ZombieHead,
        Self::ZombieArmL,
        Self::ZombieArmR,
        Self::ZombieLegL,
        Self::ZombieLegR,
    ];
    pub const MESH_COUNT: usize = Self::MESH_PARTS.len();

    /// Index of the prototype mesh shared by this part, matching `MESH_PARTS`.
    #[must_use]
    pub const fn mesh_index(self) -> usize {
        match self {
            Self::ZombieBody => 1,
            Self::ZombieHead => 2,
            Self::ZombieArmL => 3,
            Self::ZombieArmR => 4,
            Self::ZombieLegL => 5,
            Self::ZombieLegR => 6,
            _ => 0,
        }
    }
}

const ZOMBIE_PART_COUNT: usize = 6;

#[derive(Clone, Debug)]
pub struct ActorMeshLibrary {
    unit: MeshBuffer,
    zombie: [MeshBuffer; ZOMBIE_PART_COUNT],
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
        let zombie = [
            // Regenerate with `python3 scripts/generate_zombie_mesh.py`.
            zombie_mesh(ZombieMeshPart::Body),
            zombie_mesh(ZombieMeshPart::Head),
            zombie_mesh(ZombieMeshPart::ArmL),
            zombie_mesh(ZombieMeshPart::ArmR),
            zombie_mesh(ZombieMeshPart::LegL),
            zombie_mesh(ZombieMeshPart::LegR),
        ];
        Self {
            unit: b.build(),
            zombie,
        }
    }
    #[must_use]
    pub fn mesh(&self, part: ActorPart) -> &MeshBuffer {
        match part {
            ActorPart::ZombieBody => &self.zombie[0],
            ActorPart::ZombieHead => &self.zombie[1],
            ActorPart::ZombieArmL => &self.zombie[2],
            ActorPart::ZombieArmR => &self.zombie[3],
            ActorPart::ZombieLegL => &self.zombie[4],
            ActorPart::ZombieLegR => &self.zombie[5],
            _ => &self.unit,
        }
    }
}
