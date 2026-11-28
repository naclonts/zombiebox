#!/usr/bin/env python3
"""Convert downloaded skinned zombie GLBs into compact, color-baked ZBZS LOD meshes.

Dependencies used by the repository prop converter: numpy, scipy, Pillow, trimesh,
fast-simplification. Set PYTHONPATH to their environment when running this script.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct

import numpy as np
from PIL import Image
from scipy.spatial import cKDTree
import fast_simplification


TARGETS = (10065, 4973, 1836, 632)
SOURCES = {
    "bloated": "zombie-bloated.glb",
    "runner": "zombie-runner.glb",
    "soldier": "zombie-soldier.glb",
    "walker": "zombie-walker.glb",
    "woman": "zombie-woman.glb",
}
VERTEX = struct.Struct("<3f3f4B4H4H")


def glb(path):
    data = path.read_bytes()
    if data[:4] != b"glTF":
        raise ValueError(f"{path}: not a GLB")
    length, kind = struct.unpack_from("<I4s", data, 12)
    if kind != b"JSON":
        raise ValueError("GLB JSON chunk missing")
    doc = json.loads(data[20:20 + length])
    offset = 20 + length
    bin_len, bin_kind = struct.unpack_from("<I4s", data, offset)
    if bin_kind != b"BIN\0":
        raise ValueError("GLB binary chunk missing")
    return doc, data[offset + 8:offset + 8 + bin_len]


COMPONENT = {5120: (np.dtype("i1"), 1), 5121: (np.dtype("u1"), 1),
             5122: (np.dtype("<i2"), 2), 5123: (np.dtype("<u2"), 2),
             5125: (np.dtype("<u4"), 4), 5126: (np.dtype("<f4"), 4)}
SHAPE = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def accessor(doc, binary, index):
    a = doc["accessors"][index]
    bv = doc["bufferViews"][a["bufferView"]]
    dtype, size = COMPONENT[a["componentType"]]
    width = SHAPE[a["type"]]
    count = a["count"]
    stride = bv.get("byteStride", width * size)
    start = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    raw = np.ndarray((count, width), dtype=dtype, buffer=binary, offset=start,
                     strides=(stride, size)).copy()
    if a.get("normalized"):
        if dtype.kind == "u":
            raw = raw.astype(np.float32) / np.iinfo(dtype).max
        elif dtype.kind == "i":
            raw = np.maximum(raw.astype(np.float32) / np.iinfo(dtype).max, -1)
    return raw


def images(doc, binary):
    result = []
    for im in doc.get("images", []):
        bv = doc["bufferViews"][im["bufferView"]]
        start = bv.get("byteOffset", 0)
        end = start + bv["byteLength"]
        import io
        result.append(np.asarray(Image.open(io.BytesIO(binary[start:end])).convert("RGBA")))
    return result


def material_color(doc, image_cache, material_index, uv, color_attr):
    material = doc.get("materials", [])[material_index] if material_index is not None else {}
    pbr = material.get("pbrMetallicRoughness", {})
    factor = np.asarray(pbr.get("baseColorFactor", [1, 1, 1, 1]), dtype=np.float32)
    base = np.broadcast_to(factor, (len(uv), 4)).copy()
    texture = pbr.get("baseColorTexture")
    if texture:
        tex = doc["textures"][texture["index"]]
        image = image_cache[tex["source"]]
        h, w = image.shape[:2]
        # glTF UV origin and Pillow's decoded image origin are both upper left.
        px = np.clip((uv[:, 0] * w).astype(np.int64), 0, w - 1)
        py = np.clip((uv[:, 1] * h).astype(np.int64), 0, h - 1)
        texel = image[py, px].astype(np.float32) / 255.0
        rgb = texel[:, :3]
        texel[:, :3] = np.where(rgb <= 0.04045, rgb / 12.92,
                                ((rgb + 0.055) / 1.055) ** 2.4)
        base *= texel
    if color_attr is not None:
        base *= color_attr.astype(np.float32)
        if color_attr.dtype.kind in "ui":
            base /= np.iinfo(color_attr.dtype).max
    rgb = base[:, :3]
    base[:, :3] = np.where(rgb <= 0.0031308, rgb * 12.92,
                           1.055 * np.maximum(rgb, 0) ** (1 / 2.4) - 0.055)
    return np.clip(np.rint(base * 255), 0, 255).astype(np.uint8)


def mesh_data(doc, binary):
    image_cache = images(doc, binary)
    mesh_node = next(n for n in doc["nodes"] if "mesh" in n and "skin" in n)
    mesh = doc["meshes"][mesh_node["mesh"]]
    all_parts = []
    for primitive in mesh["primitives"]:
        attrs = primitive["attributes"]
        pos = accessor(doc, binary, attrs["POSITION"]).astype(np.float32)
        norm = accessor(doc, binary, attrs["NORMAL"]).astype(np.float32)
        joints = accessor(doc, binary, attrs["JOINTS_0"]).astype(np.uint16)
        weights = accessor(doc, binary, attrs["WEIGHTS_0"]).astype(np.float32)
        if "WEIGHTS_1" in attrs:
            joints = np.concatenate([joints, accessor(doc, binary, attrs["JOINTS_1"]).astype(np.uint16)], axis=1)
            weights = np.concatenate([weights, accessor(doc, binary, attrs["WEIGHTS_1"]).astype(np.float32)], axis=1)
        uv = accessor(doc, binary, attrs["TEXCOORD_0"]).astype(np.float32) if "TEXCOORD_0" in attrs else np.zeros((len(pos), 2), np.float32)
        ca = accessor(doc, binary, attrs["COLOR_0"]) if "COLOR_0" in attrs else None
        color = material_color(doc, image_cache, primitive.get("material"), uv, ca)
        ids = accessor(doc, binary, primitive["indices"]).reshape(-1).astype(np.int64)
        faces = ids.reshape(-1, 3)
        # Retain the four strongest skin influences and normalize robustly.
        order = np.argsort(weights, axis=1)[:, -4:]
        js = np.take_along_axis(joints, order, axis=1)
        ws = np.take_along_axis(weights, order, axis=1)
        ws /= np.maximum(ws.sum(axis=1, keepdims=True), 1e-8)
        all_parts.append((pos, norm, color, js, ws, faces))
    return all_parts, mesh_node


def combine(parts):
    fields = [[] for _ in range(5)]
    faces = []
    offset = 0
    for pos, norm, color, joints, weights, tri in parts:
        for i, value in enumerate((pos, norm, color, joints, weights)):
            fields[i].append(value)
        faces.append(tri + offset)
        offset += len(pos)
    return [np.concatenate(x, axis=0) for x in fields], np.concatenate(faces, axis=0)


def simplify(points, normals, colors, joints, weights, faces, target):
    if len(faces) <= target:
        return points, normals, colors, joints, weights, faces
    # GLBs duplicate vertices along UV/material seams. Weld identical position,
    # skin and color records for the geometric decimator so it can cross those
    # redundant seams without transferring attributes across real boundaries.
    weights16 = np.rint(weights * 65535).astype(np.uint16)
    key = np.concatenate([np.round(points, 6).astype(np.float32).view(np.uint32),
                          joints.astype(np.uint16), weights16, colors.astype(np.uint8)], axis=1)
    _, unique, remap = np.unique(key, axis=0, return_index=True, return_inverse=True)
    topo_faces = remap[faces]
    valid = ((topo_faces[:, 0] != topo_faces[:, 1]) &
             (topo_faces[:, 1] != topo_faces[:, 2]) &
             (topo_faces[:, 0] != topo_faces[:, 2]))
    topo_faces = topo_faces[valid]
    topo_points = points[unique]
    if len(topo_faces) <= target:
        return points, normals, colors, joints, weights, faces
    out_pos, out_faces = fast_simplification.simplify(
        np.ascontiguousarray(topo_points, dtype=np.float64),
        np.ascontiguousarray(topo_faces, dtype=np.int32), target_count=int(target), agg=25.0,
    )
    out_pos = out_pos.astype(np.float32)
    # Fast-simplification creates collapsed positions. Transfer skin and color from
    # the closest original vertex; barycentric sampling would blur color boundaries.
    nearest = cKDTree(points).query(out_pos, k=1)[1]
    out_normals = normals[nearest]
    out_colors = colors[nearest]
    out_joints = joints[nearest]
    out_weights = weights[nearest]
    length = np.linalg.norm(out_normals, axis=1, keepdims=True)
    out_normals /= np.maximum(length, 1e-8)
    out_faces = out_faces.astype(np.uint32)
    if len(out_faces) > target * 1.05:
        # QEM protects disconnected details too conservatively on a few source
        # variants. Coarsen only this already simplified mesh by spatial clusters,
        # retaining dominant joint and coarse color groups to avoid cross-limb
        # welding. Degenerate and duplicate faces disappear through remapping.
        dominant = np.argmax(out_weights, axis=1)
        bone = out_joints[np.arange(len(out_joints)), dominant]
        color_class = (out_colors[:, :3] // 8).astype(np.uint8)
        def clustered(cell, offset):
            cell_id = np.floor(out_pos / cell + offset).astype(np.int32)
            key = np.concatenate([cell_id, bone[:, None].astype(np.int32), color_class.astype(np.int32)], axis=1)
            _, first, mapping = np.unique(key, axis=0, return_index=True, return_inverse=True)
            qpos = np.zeros((len(first), 3), np.float32)
            np.add.at(qpos, mapping, out_pos)
            count = np.bincount(mapping)
            qpos /= count[:, None]
            qfaces = mapping[out_faces]
            valid = ((qfaces[:, 0] != qfaces[:, 1]) & (qfaces[:, 1] != qfaces[:, 2]) &
                     (qfaces[:, 0] != qfaces[:, 2]))
            qfaces = qfaces[valid]
            face_keys = np.sort(qfaces, axis=1)
            _, unique_face = np.unique(face_keys, axis=0, return_index=True)
            qfaces = qfaces[np.sort(unique_face)]
            return qpos, qfaces.astype(np.uint32), first
        best = None
        for offset in ([0, 0, 0], [0.25, 0, 0], [0.5, 0, 0], [0, 0.25, 0],
                       [0, 0.5, 0], [0, 0, 0.25], [0, 0, 0.5], [0.25, 0.25, 0.25],
                       [0.5, 0.5, 0.5]):
            low, high = 0.001, 0.12
            for _ in range(24):
                cell = (low + high) / 2
                qpos, qfaces, first = clustered(cell, offset)
                if best is None or abs(len(qfaces) - target) < abs(len(best[1]) - target):
                    best = (qpos, qfaces, first)
                if len(qfaces) > target:
                    low = cell
                else:
                    high = cell
        out_pos, out_faces, first = best
        if len(out_faces) > target:
            out_pos, out_faces = fast_simplification.simplify(
                np.ascontiguousarray(out_pos, dtype=np.float64),
                np.ascontiguousarray(out_faces, dtype=np.int32),
                target_count=int(target), agg=25.0,
            )
            out_pos = out_pos.astype(np.float32)
            out_faces = out_faces.astype(np.uint32)
        near = cKDTree(points).query(out_pos, k=1)[1]
        out_normals, out_colors = normals[near], colors[near]
        out_joints, out_weights = joints[near], weights[near]
        out_normals /= np.maximum(np.linalg.norm(out_normals, axis=1, keepdims=True), 1e-8)
    return out_pos, out_normals, out_colors, out_joints, out_weights, out_faces


def write_mesh(path, arrays):
    pos, normals, colors, joints, weights, faces = arrays
    if faces.max(initial=0) >= len(pos):
        raise ValueError("index out of range")
    weights = weights / np.maximum(weights.sum(axis=1, keepdims=True), 1e-9)
    wq = np.rint(weights * 65535).astype(np.uint16)
    # Correct quantization rounding so each vertex sums exactly to 65535.
    residual = 65535 - wq.sum(axis=1).astype(np.int32)
    largest = np.argmax(wq, axis=1)
    wq[np.arange(len(wq)), largest] = (
        wq[np.arange(len(wq)), largest].astype(np.int32) + residual
    ).astype(np.uint16)
    with path.open("wb") as out:
        out.write(struct.pack("<4sIIII", b"ZBZS", 1, len(pos), faces.size, 0))
        for i in range(len(pos)):
            out.write(VERTEX.pack(*pos[i], *normals[i], *colors[i], *joints[i], *wq[i]))
        out.write(faces.astype("<u4").tobytes())


def matrix(node):
    if "matrix" in node:
        return np.asarray(node["matrix"], dtype=np.float64).reshape((4, 4), order="F")
    t = np.asarray(node.get("translation", [0, 0, 0]), dtype=np.float64)
    q = np.asarray(node.get("rotation", [0, 0, 0, 1]), dtype=np.float64)
    s = np.asarray(node.get("scale", [1, 1, 1]), dtype=np.float64)
    x, y, z, w = q
    r = np.array([[1-2*y*y-2*z*z,2*x*y-2*z*w,2*x*z+2*y*w],
                  [2*x*y+2*z*w,1-2*x*x-2*z*z,2*y*z-2*x*w],
                  [2*x*z-2*y*w,2*y*z+2*x*w,1-2*x*x-2*y*y]])
    m = np.eye(4); m[:3,:3] = r @ np.diag(s); m[:3,3] = t
    return m


def skeleton(doc, mesh_node, all_parts, scale, center):
    nodes = doc["nodes"]
    skin = doc["skins"][mesh_node["skin"]]
    joints = skin["joints"]
    parent_by_child = {}
    for i, n in enumerate(nodes):
        for child in n.get("children", []):
            parent_by_child[child] = i
    root = nodes[mesh_node["skin"]] if False else None
    # Joint parent references stay inside the ordered skin joint array.
    lookup = {node_index: i for i, node_index in enumerate(joints)}
    bind_mats = [matrix(nodes[n]) for n in joints]
    inv_bind = accessor(doc, binary_from_doc(doc), skin["inverseBindMatrices"]) if False else None
    return joints, parent_by_child, lookup, bind_mats


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--source-dir", type=Path, default=Path.home() / "Downloads")
    ap.add_argument("--out", type=Path, default=Path(__file__).resolve().parents[1] / "assets/zombies/converted")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    manifest = {"format": "ZBZS v1", "targets_triangles": list(TARGETS), "assets": {}}
    for archetype, filename in SOURCES.items():
        source = args.source_dir / filename
        doc, binary = glb(source)
        parts, mesh_node = mesh_data(doc, binary)
        attrs, faces = combine(parts)
        positions, normals, colors, joints, weights = attrs
        # Apply the mesh node transform, then fit a uniform 1m height with ground at Y=0.
        node_transform = matrix(mesh_node)
        positions = (np.c_[positions, np.ones(len(positions))] @ node_transform.T)[:, :3].astype(np.float32)
        normals = (normals @ np.linalg.inv(node_transform[:3, :3])).astype(np.float32)
        normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-8)
        low, high = positions.min(axis=0), positions.max(axis=0)
        scale = 1.0 / float(high[1] - low[1])
        center = np.array([(low[0]+high[0])/2, low[1], (low[2]+high[2])/2], dtype=np.float32)
        positions = (positions - center) * scale
        # Build skeleton hierarchy and inverse binds in the same normalized model space.
        skin = doc["skins"][mesh_node["skin"]]
        skin_order = skin["joints"]
        joint_lookup = {node_index: i for i, node_index in enumerate(skin_order)}
        parent_nodes = {}
        for ni, node in enumerate(doc["nodes"]):
            for child in node.get("children", []): parent_nodes[child] = ni
        inv_binds = accessor(doc, binary, skin["inverseBindMatrices"]).reshape((-1,4,4), order="F")
        joint_entries = []
        for ji, ni in enumerate(skin_order):
            n = doc["nodes"][ni]
            parent = parent_nodes.get(ni)
            parent_index = joint_lookup.get(parent)
            local = matrix(n)
            # Scale all bind translations and conjugate inverse-bind matrices through
            # the uniform normalization and ground-center translation.
            local[:3, 3] *= scale
            if parent_index is None:
                local[:3, 3] -= center * scale
            ib = inv_binds[ji]
            c = np.eye(4); c[:3,:3] *= scale; c[:3,3] = -center * scale
            ci = np.linalg.inv(c)
            s = np.eye(4); s[:3,:3] *= scale
            ib = s @ ib @ ci
            joint_entries.append({"name": n.get("name", f"joint_{ni}"), "parent": parent_index,
                                  "bind_local": local.reshape(-1, order="F").tolist(),
                                  "inverse_bind": ib.reshape(-1, order="F").tolist()})
        skeleton_doc = {"coordinate_system": "+Y up, +Z forward", "height_m": 1.0,
                        "joint_order": "glTF skin.joints", "joints": joint_entries}
        skel_path = args.out / f"{archetype}-skeleton.json"
        skel_path.write_text(json.dumps(skeleton_doc, separators=(",", ":")))
        skel_bin_path = args.out / f"{archetype}-skeleton.zbsk"
        with skel_bin_path.open("wb") as out:
            out.write(struct.pack("<4sHH", b"ZBJS", 1, len(joint_entries)))
            for joint in joint_entries:
                name = joint["name"].encode("utf-8")
                parent = -1 if joint["parent"] is None else joint["parent"]
                out.write(struct.pack("<Hh", len(name), parent))
                out.write(name)
                out.write(struct.pack("<32f", *joint["bind_local"], *joint["inverse_bind"]))
        entry = {"title": f"{archetype.title()} Zombie", "source_filename": filename,
                 "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                 "author": "vinish.dev", "source_url": "https://vinish.dev/free-3d-models/",
                 "license": "CC BY 4.0", "license_url": "https://creativecommons.org/licenses/by/4.0/",
                 "modification": "Reduced geometry into four LODs, baked base-color textures and factors to RGBA8 vertex colors, normalized local height to 1m, and converted to ZBZS v1.",
                 "skeleton": skel_path.name, "skeleton_binary": skel_bin_path.name,
                 "joint_count": len(skin_order), "source_triangles": len(faces), "lods": []}
        entry["source_animation_clips"] = [clip.get("name", "") for clip in doc.get("animations", [])]
        for lod, target in enumerate(TARGETS):
            mesh_arrays = simplify(positions, normals, colors, joints, weights, faces, target)
            out_path = args.out / f"{archetype}-lod{lod}.zbzs"
            write_mesh(out_path, mesh_arrays)
            p = mesh_arrays[0]
            entry["lods"].append({"file": out_path.name, "sha256": hashlib.sha256(out_path.read_bytes()).hexdigest(),
                                  "vertices": len(p), "triangles": len(mesh_arrays[-1]),
                                  "bounds_min": p.min(axis=0).round(6).tolist(),
                                  "bounds_max": p.max(axis=0).round(6).tolist()})
        manifest["assets"][archetype] = entry
    (args.out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
