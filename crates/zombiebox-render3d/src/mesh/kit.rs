//! Modular, bottom-centered retail props in renderer coordinates.

use super::{FaceMask, MeshBuffer, MeshBuilder, VertexMeta, primitives::emit_box_min_max};
use glam::Vec3;

const FRAME: [f32; 4] = [0.30, 0.33, 0.31, 1.0];
const TRIM: [f32; 4] = [0.54, 0.57, 0.47, 1.0];
const BELT: [f32; 4] = [0.21, 0.23, 0.22, 1.0];

fn piece(builder: &mut MeshBuilder, min: [f32; 3], max: [f32; 3], color: [f32; 4]) {
    emit_box_min_max(
        builder,
        Vec3::from(min),
        Vec3::from(max),
        FaceMask::ALL.without(FaceMask::BOTTOM),
        color,
        VertexMeta::DEFAULT,
    );
}

/// A double-sided shelf with exposed shelves and a thin center divider.
pub fn build_gondola_shelf(length: u8) -> MeshBuffer {
    let end = f32::from(length.clamp(1, 8)) * 0.48;
    let mut mesh = MeshBuilder::new();
    piece(&mut mesh, [-end, 0.0, -0.31], [end, 0.11, 0.31], FRAME);
    piece(&mut mesh, [-end, 0.11, -0.035], [end, 1.45, 0.035], TRIM);
    for x in [-end, end - 0.04] {
        piece(&mut mesh, [x, 0.11, -0.075], [x + 0.04, 1.45, 0.075], FRAME);
    }
    for height in [0.35, 0.70, 1.05, 1.37] {
        piece(
            &mut mesh,
            [-end, height, -0.32],
            [end, height + 0.04, 0.32],
            TRIM,
        );
    }
    mesh.build()
}

/// A length-adjustable checkout counter with a recessed conveyor.
/// A till can be stacked on its front landing.
pub fn build_checkout_lane(length: u8) -> MeshBuffer {
    let end = f32::from(length.clamp(1, 8)) * 0.48;
    let mut mesh = MeshBuilder::new();
    piece(&mut mesh, [-0.38, 0.0, -end], [0.38, 0.74, end], FRAME);
    piece(
        &mut mesh,
        [-0.36, 0.74, -end],
        [0.36, 0.80, end - 0.37],
        BELT,
    );
    piece(
        &mut mesh,
        [-0.44, 0.74, end - 0.37],
        [0.44, 0.90, end],
        TRIM,
    );
    for x in [-0.44, 0.36] {
        piece(
            &mut mesh,
            [x, 0.74, -end],
            [x + 0.08, 0.86, end - 0.37],
            TRIM,
        );
    }
    mesh.build()
}
