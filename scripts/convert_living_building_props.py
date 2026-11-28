#!/usr/bin/env python3
"""Bake the living buildings shortlist into indexed, texture-free prop prototypes."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import tempfile

import fast_simplification
import numpy as np
import trimesh


ASSET_ROOT = Path(__file__).resolve().parents[1] / "assets/living-buildings"
KENNEY = "kenney/furniture-kit/Models/GLTF format"
KENNEY_PAGE = "https://kenney.nl/assets/furniture-kit"
FURNITURE = (
    "loungeSofa", "tableCoffee", "bedSingle", "bedDouble", "chair", "table",
    "kitchenCabinet", "kitchenSink", "kitchenStoveElectric", "kitchenFridge",
    "toilet", "bathtub", "bathroomSink", "televisionVintage", "desk", "chairDesk",
    "bookcaseOpen", "bookcaseClosedDoors",
)
EXTERIOR = {
    # Source ID, uniform scale, triangle target (None keeps authored topology).
    "car": ("Cz6yDaUcM9", 1.0, 1150),
    "police-car": ("BwwnUrWGmV", 1.0, 1150),
    "crate": ("3OEFd1AWfa", 3.0, 600),
    "first-aid-kit": ("Hp80p6148W", 0.2, None),
}
CAD = {
    # Source kit, meters per source unit, yaw in degrees.
    "hospital-bed": ("medical", 0.0026, -90),
    "wheelchair": ("medical", 0.001, -90),
    "iv-stand": ("medical", 0.0014, 0),
    "diagnostic-monitor": ("medical", 0.001, 0),
    "medical-cart": ("medical", 0.0013, 0),
    "assisted-checkout-till": ("retail-store", 0.001, 0),
    "shopping-cart": ("retail-store", 0.001, 0),
}
CAD_REVISION = "fbcae53585f30ab3cbc8f22f1487a40025ca3444"
VERTEX = np.dtype([("position", "<f4", 3), ("normal", "<f4", 3), ("color", "u1", 4)])


def yaw(degrees):
    return trimesh.transformations.rotation_matrix(np.deg2rad(degrees), [0, 1, 0])


def parts_from_glb(source, scale, facing, variant):
    # Trimesh assigns random names to the extra nodes of multi-material primitives.
    scene = trimesh.load_scene(source, process=False)
    parts = sorted(
        ((mesh, node, transform) for node in scene.graph.nodes_geometry
         for transform, mesh in [scene.graph[node]]),
        key=lambda part: (part[0], part[2].tobytes()),
    )
    for mesh_name, node, transform in parts:
        mesh = scene.geometry[mesh_name]
        if getattr(getattr(mesh.visual, "material", None), "baseColorTexture", None) is not None:
            raise ValueError(f"{source}: bake base-color textures before importing")
        if variant == "fridge-open" and node.startswith(("doorFridge", "doorFreezer")):
            transform = transform @ yaw(-105)
        if variant == "desk-open" and node.startswith("drawer"):
            transform = transform.copy()
            transform[2, 3] += 0.11
        # Apply inverse transpose to normals for non-uniform node transforms.
        matrix = yaw(facing) @ transform
        vertices = trimesh.transform_points(mesh.vertices, matrix) * scale
        normals = mesh.vertex_normals @ np.linalg.inv(matrix[:3, :3])
        faces = mesh.faces
        if np.linalg.det(matrix[:3, :3]) < 0:
            faces = faces[:, ::-1]
        yield vertices, normals, faces, palette(mesh)


def palette(mesh):
    material = getattr(mesh.visual, "material", None)
    factor = getattr(material, "baseColorFactor", None)
    rgb = np.ones(3) if factor is None else factor[:3] / 255
    srgb = np.where(rgb <= 0.0031308, rgb * 12.92, 1.055 * rgb ** (1 / 2.4) - 0.055)
    gray = srgb @ [0.2126, 0.7152, 0.0722]
    # Keep the source's material contrasts with slightly lower saturation.
    return np.rint(np.append(srgb * 0.8 + gray * 0.2, 1) * 255).astype("u1")


def simplify(vertices, faces, target):
    # Join position splits within this material/part so curved CAD surfaces can collapse.
    _, unique, remap = np.unique(vertices.round(7), axis=0, return_index=True, return_inverse=True)
    mesh = trimesh.Trimesh(vertices=vertices[unique], faces=remap[faces], process=False)
    mesh.update_faces(mesh.nondegenerate_faces(height=1e-7))
    if len(mesh.faces) <= target:
        return mesh.vertices, mesh.vertex_normals, mesh.faces
    vertices, faces = fast_simplification.simplify(
        np.ascontiguousarray(mesh.vertices), mesh.faces, target_count=target,
    )
    mesh = trimesh.Trimesh(vertices=vertices, faces=faces, process=False)
    mesh.update_faces(mesh.nondegenerate_faces(height=1e-7))
    mesh = trimesh.graph.smooth_shade(mesh, angle=np.deg2rad(45), facet_minarea=None)
    return mesh.vertices, mesh.vertex_normals, mesh.faces


def bake(parts, destination, pivot=None, target=None):
    source_triangles = sum(len(faces) for _, _, faces, _ in parts)
    ratio = min(1, target / source_triangles) if target else 1
    corners = []
    for vertices, normals, faces, color in parts:
        part_target = max(8, int(len(faces) * ratio))
        if part_target < len(faces):
            vertices, normals, faces = simplify(vertices, faces, part_target)
        mesh = trimesh.Trimesh(vertices=vertices, faces=faces, process=False)
        valid = mesh.nondegenerate_faces(height=1e-7)
        faces = faces[valid]
        surface = mesh.face_normals[valid, None, :]
        corner_normals = trimesh.util.unitize(normals)[faces]
        # Opposite or zero normals can occur on thin CAD pieces after decimation.
        face_aligned = np.sum(corner_normals * surface, axis=-1, keepdims=True) > 0
        record = np.empty(faces.shape, dtype=VERTEX)
        record["position"] = vertices[faces]
        record["normal"] = np.where(face_aligned, corner_normals, surface)
        record["color"] = color
        corners.append(record)

    records = np.concatenate(corners).reshape(-1)
    if pivot is None:
        low, high = records["position"].min(axis=0), records["position"].max(axis=0)
        pivot = (low + high) / 2
        pivot[1] = low[1]
    records["position"] -= pivot
    vertices, indices = np.unique(records, return_inverse=True)
    positions = vertices["position"]
    if not len(indices) or len(vertices) > 65536 or not np.isfinite(positions).all():
        raise ValueError(f"{destination}: empty, non-finite, or exceeds 16-bit index capacity")
    if not np.allclose(np.linalg.norm(vertices["normal"], axis=1), 1, atol=1e-5):
        raise ValueError(f"{destination}: invalid vertex normal")
    destination.write_bytes(
        struct.pack("<4s2I", b"ZBPM", len(vertices), indices.size)
        + vertices.tobytes() + indices.astype("<u2").tobytes()
    )
    stats = {
        "source_triangles": source_triangles,
        "triangles": indices.size // 3,
        "vertices": len(vertices),
        "bounds_min_simulation_xyz_m": [round(float(v), 5) for v in positions[:, [0, 2, 1]].min(axis=0)],
        "bounds_max_simulation_xyz_m": [round(float(v), 5) for v in positions[:, [0, 2, 1]].max(axis=0)],
    }
    return pivot, stats


CAD_PARAMETER = re.compile(r"param\('([^']+)',\s*('[^']*'|[\d.]+), (.*?)\);")


def export_cad(source, directory, command, state):
    # The published 0.15 CLI predates catalog metadata and string parameters.
    # Resolve the shortlist's literal defaults in a disposable copy.
    def default(match):
        name, value, options = match.groups()
        if name == "drawer" and state == "closed":
            value = "'shut'"
        return f"{{ value: {value} }};" if "choices:" in options else f"{value};"

    project = directory / source.name
    source_text = re.sub(r"\bcylinder\(", "polygonCylinder(", source.read_text())
    if source.stem == "shopping-cart.kcad":
        source_text = source_text.replace(
            "turned([[0, 0], [22, 0], [22, 18], [0, 18]])", "polygonCylinder(18, 22, 8)"
        )
    project.write_text(
        "const meta = (_value: unknown) => {};\nconst presets = (_value: unknown) => {};\n"
        # GLB export retessellates analytic cylinders. Use the authored segment count.
        "const polygonCylinder = (height: number, radius: number, segments: number) =>\n"
        "  extrudePolygon(Array.from({length: segments}, (_, i) => {\n"
        "    const angle = 2 * Math.PI * i / segments;\n"
        "    return [radius * Math.cos(angle), radius * Math.sin(angle)];\n"
        "  }), height);\n"
        + CAD_PARAMETER.sub(default, source_text)
    )
    glb = directory / "asset.glb"
    subprocess.run([*command, "export", "glb", str(project), "--out", str(glb)], check=True)
    return glb


def convert(root, out, kernelcad):
    out.mkdir(parents=True, exist_ok=True)
    manifest = {}

    def write(asset, state, source, author, url, scale, facing=0, target=None,
              variant=None, pivot=None, glb=None, version=None):
        filename = f"{asset}-{state}.mesh" if state else f"{asset}.mesh"
        parts = list(parts_from_glb(glb or source, scale, facing, variant))
        pivot, stats = bake(parts, out / filename, pivot, target)
        manifest[filename] = {
            "asset": asset, "state": state, "source": source.relative_to(root).as_posix(),
            "author": author, "url": url, "license": "CC0-1.0", "source_version": version,
            "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "mesh_sha256": hashlib.sha256((out / filename).read_bytes()).hexdigest(),
            "scale": scale, "source_yaw_degrees": facing,
            "target_triangles": target, "variant_edit": variant, **stats,
        }
        return pivot

    for stem in FURNITURE:
        source = root / KENNEY / f"{stem}.glb"
        asset = re.sub(r"(?<!^)(?=[A-Z])", "-", stem).lower()
        state = "closed" if stem in ("kitchenFridge", "desk") else None
        base = write(asset, state, source, "Kenney", KENNEY_PAGE, 2.0,
                     version="Furniture Kit 2.0")
        if state == "closed":
            variant = "fridge-open" if stem == "kitchenFridge" else "desk-open"
            write(asset, "opened", source, "Kenney", KENNEY_PAGE, 2.0,
                  variant=variant, pivot=base, version="Furniture Kit 2.0")
    box_pivot = None
    for state, stem in (("closed", "cardboardBoxClosed"), ("opened", "cardboardBoxOpen")):
        box_pivot = write("cardboard-box", state, root / KENNEY / f"{stem}.glb",
                          "Kenney", KENNEY_PAGE, 2.0, pivot=box_pivot,
                          version="Furniture Kit 2.0")

    for name, (slug, scale, target) in EXTERIOR.items():
        write(name, None, root / "poly-pizza" / f"{name}.glb", "Quaternius",
              f"https://poly.pizza/m/{slug}", scale, target=target)

    with tempfile.TemporaryDirectory() as tmp:
        for name, (kit, scale, facing) in CAD.items():
            folder = "medical/kernelcad-source" if kit == "medical" else "retail"
            source = root / folder / f"{name}.kcad.ts"
            pivot = None
            for state in (("closed", "opened") if name == "assisted-checkout-till" else (None,)):
                glb = export_cad(source, Path(tmp), kernelcad, state)
                pivot = write(name, state, source, "kernelCAD",
                              f"https://kernelcad.com/catalog/{kit}/{name}/",
                              scale, facing, target=1200, pivot=pivot, glb=glb,
                              version=CAD_REVISION)

    (out / "manifest.json").write_text(
        json.dumps({"format_version": 1, "prototypes": manifest}, indent=2, sort_keys=True) + "\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=ASSET_ROOT)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--kernelcad", nargs="+", default=["kernelcad"])
    args = parser.parse_args()
    output = args.out or args.source / "converted"
    output.parent.mkdir(parents=True, exist_ok=True)
    # Complete every export before publishing files from the build.
    with tempfile.TemporaryDirectory(dir=output.parent) as staging:
        staged = Path(staging)
        convert(args.source, staged, args.kernelcad)
        output.mkdir(exist_ok=True)
        for mesh in staged.glob("*.mesh"):
            mesh.replace(output / mesh.name)
        (staged / "manifest.json").replace(output / "manifest.json")
