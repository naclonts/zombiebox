//! Generate a self-contained review scene from the same prototypes as the game.
//! Run with `cargo run -p zombiebox-render3d --example prop_gallery -- [output.html]`.

use std::{collections::BTreeMap, fmt::Write as _, fs, path::PathBuf};

use glam::{Mat4, Vec3};
use zombiebox_core::{PropId, Vec2};
use zombiebox_render3d::{
    CameraConfig, CameraMode, CameraRig, CameraTarget, MaterialFlags, MeshBuffer, PropMeshLibrary,
    SimVec3, VertexMeta,
};

const TEMPLATE: &str = include_str!("gallery.html");
const MANIFEST: &str = include_str!("../../../../assets/living-buildings/converted/manifest.json");
const WIDTH: u32 = 256;
const HEIGHT: u32 = 176;

fn category(name: &str) -> &'static str {
    match name {
        "bed-single" | "bed-double" | "lounge-sofa" | "chair" | "table" | "table-coffee"
        | "television-vintage" => "Living and bedrooms",
        "bathroom-sink"
        | "bathtub"
        | "toilet"
        | "kitchen-fridge"
        | "kitchen-sink"
        | "kitchen-stove-electric"
        | "kitchen-cabinet" => "Kitchen and bath",
        "desk" | "chair-desk" | "bookcase-open" | "bookcase-closed-doors" => "Office",
        "hospital-bed" | "diagnostic-monitor" | "iv-stand" | "medical-cart" | "wheelchair"
        | "first-aid-kit" => "Medical",
        "shopping-cart" | "assisted-checkout-till" | "checkout-lane" | "gondola-shelf" => "Retail",
        "car" | "police-car" => "Exterior",
        "sign" => "Signage",
        _ => "Storage",
    }
}

fn variants(name: &str) -> (&str, &str) {
    if let Some(base) = name.strip_suffix("-opened") {
        (base, "Opened")
    } else if let Some(base) = name.strip_suffix("-closed") {
        (base, "Closed")
    } else {
        (name, "Standard")
    }
}

fn gallery_cameras(meshes: &[(&str, &str, &MeshBuffer)]) -> [Mat4; CameraMode::COUNT] {
    let mut min = Vec3::ZERO;
    let mut max = Vec3::ZERO;
    for aabb in meshes.iter().filter_map(|(_, _, mesh)| mesh.compute_aabb()) {
        min = min.min(aabb.min);
        max = max.max(aabb.max);
    }
    let height = max.y - min.y;
    let diameter = (max - min).length().max(0.85);
    let config = CameraConfig {
        look_ahead_enabled: false,
        iso_pixels_per_unit: HEIGHT as f32 / (1.28 * diameter),
        iso_focus_height: min.y + height * 0.5,
        overhead_distance: diameter * 1.5,
        orbit_default_distance: diameter * 1.35,
        orbit_min_distance: diameter * 0.5,
        orbit_focus_height: min.y + height * 0.5,
        chase_distance: diameter * 1.45,
        chase_focus_height: min.y + height * 0.46,
        chase_forward_look: 0.0,
        first_person_eye_height: min.y + height * 0.65,
        ..CameraConfig::default()
    };
    CameraMode::ALL.map(|mode| {
        let mut rig = CameraRig::with_config(WIDTH, HEIGHT, config);
        rig.set_mode(mode);
        let mut pos = SimVec3::ZERO;
        if mode == CameraMode::FirstPerson {
            pos.y = diameter * 1.3;
        }
        let target = CameraTarget::from_sim_pose(pos, Vec2::new(0.0, -1.0), None);
        rig.initialize_view_from_heading(target.heading);
        rig.update(target, 0.0).view_proj
    })
}

fn write_mesh(out: &mut String, mesh: &MeshBuffer) {
    out.push('[');
    for vertex in &mesh.vertices {
        let [x, y, z] = vertex.position;
        let [nx, ny, nz] = vertex.normal;
        let [r, g, b, _] = vertex.color;
        let unlit = VertexMeta::unpack(vertex.meta)
            .flags
            .contains(MaterialFlags::UNLIT);
        write!(
            out,
            "{x:.6},{y:.6},{z:.6},{nx:.6},{ny:.6},{nz:.6},{r:.6},{g:.6},{b:.6},{},",
            u8::from(unlit)
        )
        .unwrap();
    }
    out.push(']');
}

fn gallery() -> String {
    let library = PropMeshLibrary::builtin();
    let mut rows = BTreeMap::<_, Vec<_>>::new();
    for (filename, prototype) in library.iter() {
        let (name, state) = variants(filename);
        rows.entry((category(name), name))
            .or_default()
            .push((filename, state, &prototype.mesh));
    }
    let mut data = format!(
        "const manifest = {};\nconst cameraNames = {:?};\n",
        MANIFEST.replace('<', "\\u003c"),
        CameraMode::ALL.map(CameraMode::label),
    );
    data.push_str("const facings = {");
    for (dir, angle) in [
        ("North", std::f32::consts::PI),
        ("East", std::f32::consts::FRAC_PI_2),
        ("South", 0.0),
        ("West", -std::f32::consts::FRAC_PI_2),
    ] {
        write!(
            data,
            "{dir}: {:?},",
            Mat4::from_rotation_y(angle).to_cols_array()
        )
        .unwrap();
    }
    data.push_str("};\nconst palettes = {Residential: [1, 0.95, 0.88], Clinical: [0.88, 0.96, 1], Distressed: [0.80, 0.77, 0.72]};\nconst props = [\n");
    for ((category, name), meshes) in rows {
        let cameras = gallery_cameras(&meshes).map(|mat| mat.to_cols_array());
        let id = PropId::from_stem(name).expect("gallery mesh has no gameplay definition");
        let def = id.def();
        let lo = def.collision_min;
        let hi = def.collision_max;
        let bounds = [lo.x, lo.z, lo.y, hi.x, hi.z, hi.y];
        let numeric_id = id as u16;
        write!(
            data,
            "{{name:{name:?},id:{numeric_id},category:{category:?},cameras:{cameras:?},bounds:{bounds:?},states:[",
        )
        .unwrap();
        for (filename, state, mesh) in meshes {
            let suffix = if state == "Standard" { "" } else { state };
            write!(
                data,
                "{{id:{filename:?},name:{suffix:?},source:manifest.prototypes['{filename}.mesh'],vertices:"
            )
            .unwrap();
            write_mesh(&mut data, mesh);
            write!(data, ",indices:{:?}}},", mesh.indices).unwrap();
        }
        data.push_str("]},\n");
    }
    data.push_str("];");
    TEMPLATE.replace(
        "<script id=\"gallery-data\"></script>",
        &format!("<script id=\"gallery-data\">\n{data}\n</script>"),
    )
}

fn main() -> std::io::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("target/prop-gallery.html"), PathBuf::from);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, gallery())?;
    println!("Open {}", path.canonicalize()?.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_meshes_fit_their_gameplay_envelopes() {
        const EPSILON: f32 = 0.0001;
        let library = PropMeshLibrary::builtin();
        let mut checked = 0;
        for (filename, prototype) in library.iter() {
            let (name, state) = variants(filename);
            if !MANIFEST.contains(&format!("\"{filename}.mesh\"")) {
                continue;
            }
            let def = PropId::from_stem(name).unwrap().def();
            let bounds = prototype.mesh.compute_aabb().unwrap();
            assert!(bounds.min.y.abs() < 0.0001, "{filename} is off its support");
            assert!(
                bounds.min.x >= -f32::from(def.span.width) * 0.5 - EPSILON
                    && bounds.max.x <= f32::from(def.span.width) * 0.5 + EPSILON
                    && bounds.min.z >= -f32::from(def.span.height) * 0.5 - EPSILON
                    && bounds.max.z <= f32::from(def.span.height) * 0.5 + EPSILON
                    && bounds.max.y <= def.collision_max.z + EPSILON,
                "{filename}: {bounds:?} outside {:?}",
                def.span,
            );
            if state != "Opened" {
                let lo = def.collision_min;
                let hi = def.collision_max;
                assert!(
                    bounds
                        .min
                        .cmpge(Vec3::new(lo.x, lo.z, lo.y) - Vec3::splat(EPSILON))
                        .all()
                        && bounds
                            .max
                            .cmple(Vec3::new(hi.x, hi.z, hi.y) + Vec3::splat(EPSILON))
                            .all(),
                    "{filename}: closed geometry outside collision box"
                );
            }
            assert!(prototype.mesh.vertices.len() <= usize::from(u16::MAX) + 1);
            checked += 1;
        }
        assert_eq!(checked, 34);
    }
}
