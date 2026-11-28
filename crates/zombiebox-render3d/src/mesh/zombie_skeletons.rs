//! Generated Mixamo skeleton palettes matching the bundled ZBZS zombie meshes.
use crate::animation::zombie::{ZombieJoint, ZombieSkeleton};
use glam::Mat4;

const BLOATED: &[u8] = include_bytes!("../../../../assets/zombies/converted/bloated-skeleton.zbsk");
const RUNNER: &[u8] = include_bytes!("../../../../assets/zombies/converted/runner-skeleton.zbsk");
const SOLDIER: &[u8] = include_bytes!("../../../../assets/zombies/converted/soldier-skeleton.zbsk");
const WALKER: &[u8] = include_bytes!("../../../../assets/zombies/converted/walker-skeleton.zbsk");
const WOMAN: &[u8] = include_bytes!("../../../../assets/zombies/converted/woman-skeleton.zbsk");

/// Return all source archetypes keyed by the manifest archetype name.
#[must_use]
pub fn all_zombie_skeletons() -> [(&'static str, ZombieSkeleton); 5] {
    [
        ("bloated", parse(BLOATED)),
        ("runner", parse(RUNNER)),
        ("soldier", parse(SOLDIER)),
        ("walker", parse(WALKER)),
        ("woman", parse(WOMAN)),
    ]
}

fn parse(bytes: &[u8]) -> ZombieSkeleton {
    assert!(
        bytes.len() >= 8 && &bytes[..4] == b"ZBJS",
        "invalid generated zombie skeleton"
    );
    assert_eq!(
        u16_at(bytes, 4),
        1,
        "unsupported generated zombie skeleton version"
    );
    let count = usize::from(u16_at(bytes, 6));
    let mut cursor = 8;
    let mut joints = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = usize::from(u16_at(bytes, cursor));
        let parent = i16_at(bytes, cursor + 2);
        cursor += 4;
        let name = std::str::from_utf8(&bytes[cursor..cursor + name_len])
            .expect("generated joint name is UTF-8")
            .to_owned();
        cursor += name_len;
        let bind_local = matrix(bytes, &mut cursor);
        let inverse_bind = matrix(bytes, &mut cursor);
        joints.push(ZombieJoint {
            name,
            parent: (parent >= 0).then_some(parent as usize),
            bind_local,
            inverse_bind,
        });
    }
    assert_eq!(
        cursor,
        bytes.len(),
        "trailing data in generated zombie skeleton"
    );
    ZombieSkeleton { joints }
}

fn matrix(bytes: &[u8], cursor: &mut usize) -> Mat4 {
    let mut columns = [0.0_f32; 16];
    for value in &mut columns {
        *value = f32::from_le_bytes(bytes[*cursor..*cursor + 4].try_into().unwrap());
        *cursor += 4;
    }
    Mat4::from_cols_array(&columns)
}

fn u16_at(bytes: &[u8], cursor: usize) -> u16 {
    u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap())
}

fn i16_at(bytes: &[u8], cursor: usize) -> i16 {
    i16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap())
}
