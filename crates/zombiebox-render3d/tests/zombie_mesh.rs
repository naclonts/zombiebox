//! Validates the generated shambler mesh data committed in `mesh/zombie.rs`.

use zombiebox_render3d::mesh::zombie::{ZombieMeshPart, zombie_mesh};

#[test]
fn generated_parts_validate_and_carry_authored_colors() {
    for part in [
        ZombieMeshPart::Body,
        ZombieMeshPart::Head,
        ZombieMeshPart::ArmL,
        ZombieMeshPart::ArmR,
        ZombieMeshPart::LegL,
        ZombieMeshPart::LegR,
    ] {
        let mesh = zombie_mesh(part);
        assert!(!mesh.is_empty(), "{part:?} must contain triangles");
        mesh.validate()
            .unwrap_or_else(|error| panic!("{part:?} mesh invalid: {error}"));
        assert!(
            mesh.vertices.iter().any(|vertex| vertex.color != [1.0; 4]),
            "{part:?} must keep its generated vertex palette"
        );
    }
}
