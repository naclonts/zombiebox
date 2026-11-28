//! Bottom-centered pixel relief for physical signs, facing renderer +Z.

use super::{
    FaceMask, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, primitives::emit_box_min_max,
};
use glam::Vec3;

// Bit 0 is the rightmost column of each row.
const GLYPHS: [[u8; 7]; 43] = [
    [14, 17, 17, 31, 17, 17, 17], // A
    [30, 17, 17, 30, 17, 17, 30], // B
    [14, 17, 16, 16, 16, 17, 14], // C
    [30, 17, 17, 17, 17, 17, 30], // D
    [31, 16, 16, 30, 16, 16, 31], // E
    [31, 16, 16, 30, 16, 16, 16], // F
    [14, 17, 16, 23, 17, 17, 14], // G
    [17, 17, 17, 31, 17, 17, 17], // H
    [31, 4, 4, 4, 4, 4, 31],      // I
    [7, 2, 2, 2, 18, 18, 12],     // J
    [17, 18, 20, 24, 20, 18, 17], // K
    [16, 16, 16, 16, 16, 16, 31], // L
    [17, 27, 21, 17, 17, 17, 17], // M
    [17, 25, 21, 19, 17, 17, 17], // N
    [14, 17, 17, 17, 17, 17, 14], // O
    [30, 17, 17, 30, 16, 16, 16], // P
    [14, 17, 17, 17, 21, 18, 13], // Q
    [30, 17, 17, 30, 20, 18, 17], // R
    [15, 16, 16, 14, 1, 1, 30],   // S
    [31, 4, 4, 4, 4, 4, 4],       // T
    [17, 17, 17, 17, 17, 17, 14], // U
    [17, 17, 17, 17, 17, 10, 4],  // V
    [17, 17, 17, 17, 21, 27, 17], // W
    [17, 17, 10, 4, 10, 17, 17],  // X
    [17, 17, 10, 4, 4, 4, 4],     // Y
    [31, 1, 2, 4, 8, 16, 31],     // Z
    [14, 17, 19, 21, 25, 17, 14], // 0
    [4, 12, 4, 4, 4, 4, 14],      // 1
    [14, 17, 1, 2, 4, 8, 31],     // 2
    [30, 1, 1, 14, 1, 1, 30],     // 3
    [2, 6, 10, 18, 31, 2, 2],     // 4
    [31, 16, 16, 30, 1, 1, 30],   // 5
    [14, 16, 16, 30, 17, 17, 14], // 6
    [31, 1, 2, 4, 8, 8, 8],       // 7
    [14, 17, 17, 14, 17, 17, 14], // 8
    [14, 17, 17, 15, 1, 1, 14],   // 9
    [0, 4, 8, 31, 8, 4, 0],       // ←
    [0, 4, 2, 31, 2, 4, 0],       // →
    [4, 14, 21, 4, 4, 4, 0],      // ↑
    [0, 4, 4, 4, 21, 14, 4],      // ↓
    [0, 0, 4, 14, 4, 0, 0],       // +
    [0, 0, 0, 14, 0, 0, 0],       // -
    [14, 17, 1, 6, 4, 0, 4],      // ?
];

fn glyph(ch: char) -> [u8; 7] {
    GLYPHS[match ch.to_ascii_uppercase() {
        'A'..='Z' => ch.to_ascii_uppercase() as usize - b'A' as usize,
        '0'..='9' => 26 + ch as usize - b'0' as usize,
        '←' | '<' => 36,
        '→' | '>' => 37,
        '↑' => 38,
        '↓' => 39,
        '+' => 40,
        '-' => 41,
        ' ' => return [0; 7],
        _ => 42,
    }]
}

#[derive(Clone, Copy, Debug)]
pub enum Pictogram {
    Restroom,
    Pharmacy,
    Police,
    Stairs,
    Elevator,
}

impl Pictogram {
    fn pixels(self) -> [u8; 8] {
        match self {
            Self::Restroom => [0x42, 0x81, 0xe7, 0x66, 0x66, 0xef, 0x52, 0x52],
            Self::Pharmacy => [0x00, 0x18, 0x18, 0x7e, 0x7e, 0x18, 0x18, 0x00],
            Self::Police => [0x7e, 0x81, 0x99, 0xbd, 0x99, 0x42, 0x24, 0x18],
            Self::Stairs => [0x00, 0x07, 0x04, 0x1c, 0x10, 0x70, 0x40, 0xc0],
            Self::Elevator => [0x7e, 0x42, 0x52, 0x7a, 0x52, 0x52, 0x42, 0x7e],
        }
    }
}

const PANEL: [f32; 4] = [0.11, 0.17, 0.20, 1.0];
const INK: [f32; 4] = [0.86, 0.84, 0.63, 1.0];
const FACE: f32 = 0.016;

fn panel(width: f32, height: f32) -> MeshBuilder {
    let mut mesh = MeshBuilder::new();
    emit_box_min_max(
        &mut mesh,
        Vec3::new(-width * 0.5, 0.0, -FACE),
        Vec3::new(width * 0.5, height, FACE),
        FaceMask::ALL,
        PANEL,
        VertexMeta::DEFAULT,
    );
    mesh
}

fn relief(mesh: &mut MeshBuilder, mut rows: [u8; 8], width: usize, origin: [f32; 2], cell: f32) {
    // Greedily merge maximal runs, then extend across neighboring rows.
    for y in 0..8 {
        let mut x = 0;
        while x < width {
            let mark = |column| 1u8 << (width - 1 - column);
            if rows[y] & mark(x) == 0 {
                x += 1;
                continue;
            }
            let start = x;
            let mut run = 0;
            while x < width && rows[y] & mark(x) != 0 {
                run |= mark(x);
                x += 1;
            }
            let mut end = y;
            while end < 8 && rows[end] & run == run {
                rows[end] &= !run;
                end += 1;
            }
            emit_box_min_max(
                mesh,
                Vec3::new(
                    origin[0] + start as f32 * cell,
                    origin[1] - end as f32 * cell,
                    FACE,
                ),
                Vec3::new(
                    origin[0] + x as f32 * cell,
                    origin[1] - y as f32 * cell,
                    FACE + cell * 0.16,
                ),
                FaceMask::ALL.without(FaceMask::NEG_Z),
                INK,
                VertexMeta::DEFAULT.with_flags(MaterialFlags::UNLIT),
            );
        }
    }
}

pub fn build_sign(text: &str, max_width: f32) -> MeshBuffer {
    let text: Vec<_> = text.chars().take(32).collect();
    let columns = text.len().max(1) * 6 + 1;
    let cell = (max_width / columns as f32).min(0.04);
    let width = columns as f32 * cell;
    let mut mesh = panel(width, 9.0 * cell);
    for (i, ch) in text.into_iter().enumerate() {
        let mut rows = [0; 8];
        rows[..7].copy_from_slice(&glyph(ch));
        relief(
            &mut mesh,
            rows,
            5,
            [-width * 0.5 + (i * 6 + 1) as f32 * cell, 8.0 * cell],
            cell,
        );
    }
    mesh.build()
}

pub fn build_pictogram(icon: Pictogram) -> MeshBuffer {
    let mut mesh = panel(0.4, 0.4);
    relief(&mut mesh, icon.pixels(), 8, [-0.16, 0.36], 0.04);
    mesh.build()
}
