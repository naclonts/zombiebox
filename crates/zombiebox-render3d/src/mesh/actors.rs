//! Shared NPC prototypes plus authored survivor and weapon meshes.
use super::{
    FaceMask, GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, emit_cuboid,
    survivor::{SurvivorNode, survivor_mesh},
    weapons::{WeaponModel, weapon_mesh, weapon_nodes},
    zombie::{ZombieMeshPart, zombie_mesh},
};
use glam::{Mat4, Vec3};

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
    Survivor(SurvivorNode),
}

const SURVIVOR_RENDER_NODES: [SurvivorNode; 15] = [
    SurvivorNode::Pelvis,
    SurvivorNode::Torso,
    SurvivorNode::Head,
    SurvivorNode::UpperArmL,
    SurvivorNode::ForearmL,
    SurvivorNode::HandL,
    SurvivorNode::ThighL,
    SurvivorNode::ShinL,
    SurvivorNode::FootL,
    SurvivorNode::UpperArmR,
    SurvivorNode::ForearmR,
    SurvivorNode::HandR,
    SurvivorNode::ThighR,
    SurvivorNode::ShinR,
    SurvivorNode::FootR,
];
const ACTOR_PARTS: [ActorPart; 26] = [
    ActorPart::Body,
    ActorPart::ZombieBody,
    ActorPart::ZombieHead,
    ActorPart::ZombieArmL,
    ActorPart::ZombieArmR,
    ActorPart::ZombieLegL,
    ActorPart::ZombieLegR,
    ActorPart::Survivor(SurvivorNode::Pelvis),
    ActorPart::Survivor(SurvivorNode::Torso),
    ActorPart::Survivor(SurvivorNode::Head),
    ActorPart::Survivor(SurvivorNode::UpperArmL),
    ActorPart::Survivor(SurvivorNode::ForearmL),
    ActorPart::Survivor(SurvivorNode::HandL),
    ActorPart::Survivor(SurvivorNode::ThighL),
    ActorPart::Survivor(SurvivorNode::ShinL),
    ActorPart::Survivor(SurvivorNode::FootL),
    ActorPart::Survivor(SurvivorNode::UpperArmR),
    ActorPart::Survivor(SurvivorNode::ForearmR),
    ActorPart::Survivor(SurvivorNode::HandR),
    ActorPart::Survivor(SurvivorNode::ThighR),
    ActorPart::Survivor(SurvivorNode::ShinR),
    ActorPart::Survivor(SurvivorNode::FootR),
    ActorPart::Axe,
    ActorPart::Pistol,
    ActorPart::Shotgun,
    ActorPart::RocketLauncher,
];
impl ActorPart {
    pub const MESH_PARTS: [Self; 26] = ACTOR_PARTS;
    pub const MESH_COUNT: usize = Self::MESH_PARTS.len();
    #[must_use]
    pub fn mesh_index(self) -> usize {
        Self::MESH_PARTS
            .iter()
            .position(|p| *p == self)
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
pub struct ActorMeshLibrary {
    unit: MeshBuffer,
    zombie: [MeshBuffer; 6],
    survivor: Vec<(SurvivorNode, MeshBuffer)>,
    weapons: Vec<(ActorPart, MeshBuffer)>,
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
            zombie_mesh(ZombieMeshPart::Body),
            zombie_mesh(ZombieMeshPart::Head),
            zombie_mesh(ZombieMeshPart::ArmL),
            zombie_mesh(ZombieMeshPart::ArmR),
            zombie_mesh(ZombieMeshPart::LegL),
            zombie_mesh(ZombieMeshPart::LegR),
        ];
        let survivor = SURVIVOR_RENDER_NODES
            .into_iter()
            .filter_map(|n| survivor_mesh(n).map(|m| (n, m)))
            .collect();
        let weapons = [
            (WeaponModel::Axe, ActorPart::Axe),
            (WeaponModel::Pistol, ActorPart::Pistol),
            (WeaponModel::Shotgun, ActorPart::Shotgun),
            (WeaponModel::RocketLauncher, ActorPart::RocketLauncher),
        ]
        .into_iter()
        .map(|(model, part)| {
            let mut mesh = MeshBuffer::new();
            let mut transforms = std::collections::BTreeMap::<&str, Mat4>::new();
            for node in weapon_nodes(model) {
                let local = Mat4::from_translation(Vec3::from_array(node.translation));
                let transform = node
                    .parent
                    .and_then(|parent| transforms.get(parent).copied())
                    .map_or(local, |parent| parent * local);
                transforms.insert(node.name, transform);
                if node.has_mesh
                    && let Some(part_mesh) = weapon_mesh(model, node.name)
                {
                    mesh.append_transformed(&part_mesh, transform);
                }
            }
            (part, mesh)
        })
        .collect();
        Self {
            unit: b.build(),
            zombie,
            survivor,
            weapons,
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
            ActorPart::Survivor(n) => self
                .survivor
                .iter()
                .find(|(node, _)| *node == n)
                .map(|(_, m)| m)
                .unwrap_or(&self.unit),
            ActorPart::Axe | ActorPart::Pistol | ActorPart::Shotgun | ActorPart::RocketLauncher => {
                self.weapons
                    .iter()
                    .find(|(p, _)| *p == part)
                    .map(|(_, m)| m)
                    .unwrap_or(&self.unit)
            }
            _ => &self.unit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_survivor_and_gameplay_weapon_prototypes_are_registered() {
        let library = ActorMeshLibrary::new();
        let unit_vertex_count = library.mesh(ActorPart::Body).vertices.len();
        for node in SURVIVOR_RENDER_NODES {
            let mesh = library.mesh(ActorPart::Survivor(node));
            assert!(
                mesh.vertices.len() > unit_vertex_count,
                "{} fell back to the cuboid",
                node.name()
            );
            assert!(mesh.validate().is_ok(), "{}", node.name());
        }
        for part in [
            ActorPart::Axe,
            ActorPart::Pistol,
            ActorPart::Shotgun,
            ActorPart::RocketLauncher,
        ] {
            let mesh = library.mesh(part);
            assert!(
                mesh.vertices.len() > unit_vertex_count,
                "{part:?} fell back to the cuboid"
            );
            assert!(mesh.validate().is_ok(), "{part:?}");
        }
        assert_eq!(ActorPart::MESH_PARTS.len(), ActorPart::MESH_COUNT);
        assert_eq!(
            ActorPart::MESH_PARTS
                .iter()
                .map(|part| part.mesh_index())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            ActorPart::MESH_COUNT
        );
    }
}
