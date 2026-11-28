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

const SCREEN: [f32; 4] = [0.10, 0.16, 0.15, 1.0];
const PLASTIC: [f32; 4] = [0.44, 0.40, 0.33, 1.0];

/// A low two-door TV console. Footprint and height match the `TvStand`
/// collider (1.20 m × 0.44 m, 0.42 m tall). Front is local +Z.
pub fn build_tv_stand() -> MeshBuffer {
    let mut mesh = MeshBuilder::new();
    piece(&mut mesh, [-0.56, 0.0, -0.18], [0.56, 0.04, 0.18], BELT);
    piece(&mut mesh, [-0.58, 0.04, -0.20], [0.58, 0.38, 0.20], FRAME);
    piece(&mut mesh, [-0.60, 0.38, -0.22], [0.60, 0.42, 0.22], TRIM);
    for (min_x, max_x) in [(-0.56, -0.03), (0.03, 0.56)] {
        piece(&mut mesh, [min_x, 0.08, 0.19], [max_x, 0.34, 0.22], TRIM);
    }
    for x in [-0.09, 0.07] {
        piece(&mut mesh, [x, 0.19, 0.22], [x + 0.02, 0.23, 0.235], BELT);
    }
    mesh.build()
}

/// A bedside table with two drawers and a knob on each. Footprint and height
/// match the `Nightstand` collider (0.52 m × 0.48 m, 0.58 m tall).
pub fn build_nightstand() -> MeshBuffer {
    let mut mesh = MeshBuilder::new();
    for (x, z) in [(-0.22, -0.20), (0.18, -0.20), (-0.22, 0.16), (0.18, 0.16)] {
        piece(&mut mesh, [x, 0.0, z], [x + 0.04, 0.06, z + 0.04], BELT);
    }
    piece(&mut mesh, [-0.24, 0.06, -0.22], [0.24, 0.56, 0.22], FRAME);
    piece(&mut mesh, [-0.26, 0.56, -0.24], [0.26, 0.58, 0.24], TRIM);
    for (min_y, max_y) in [(0.36, 0.52), (0.12, 0.32)] {
        piece(&mut mesh, [-0.21, min_y, 0.22], [0.21, max_y, 0.235], TRIM);
        let knob_y = (min_y + max_y) * 0.5;
        piece(
            &mut mesh,
            [-0.03, knob_y - 0.02, 0.235],
            [0.03, knob_y + 0.02, 0.255],
            BELT,
        );
    }
    mesh.build()
}

/// A vintage CRT on a short plinth. Footprint and height match the
/// `Television` collider (0.82 m × 0.54 m, 0.54 m tall) so it sits on the
/// `TvStand` top. Front is local +Z.
pub fn build_crt_television() -> MeshBuffer {
    let mut mesh = MeshBuilder::new();
    for x in [-0.34, 0.30] {
        piece(&mut mesh, [x, 0.0, -0.18], [x + 0.04, 0.03, -0.14], BELT);
    }
    piece(&mut mesh, [-0.41, 0.03, -0.27], [0.41, 0.50, 0.24], PLASTIC);
    piece(&mut mesh, [-0.36, 0.08, 0.24], [0.36, 0.46, 0.26], FRAME);
    piece(&mut mesh, [-0.29, 0.13, 0.26], [0.29, 0.41, 0.265], SCREEN);
    piece(&mut mesh, [0.31, 0.36, 0.25], [0.37, 0.44, 0.27], BELT);
    piece(&mut mesh, [0.31, 0.20, 0.25], [0.37, 0.27, 0.27], BELT);
    mesh.build()
}
