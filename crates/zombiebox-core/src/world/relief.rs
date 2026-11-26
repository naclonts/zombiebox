//! Deterministic 5-stage continuous county relief, flat building pads, road grade clamping, and waterbeds.

use super::geography::ValleyMeta;
use super::sites::PlannedPad;
use super::{Ground, Position, Tile, WORLD_SIZE, Water, noise};
use crate::spatial::{MIN_BRIDGE_CLEARANCE, TERRAIN_GRID_SIDE, TerrainGrid, WATER_SURFACE_Z};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

const GRID_LEN: usize = TERRAIN_GRID_SIDE * TERRAIN_GRID_SIDE;
const ROAD_LIPSCHITZ_STEP: f32 = 0.130;

#[inline]
fn grid_idx(gx: i32, gy: i32) -> usize {
    gy as usize * TERRAIN_GRID_SIDE + gx as usize
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn f32_to_ord_u32(v: f32) -> u32 {
    let bits = v.to_bits();
    if bits & 0x8000_0000 != 0 {
        !bits
    } else {
        bits ^ 0x8000_0000
    }
}

struct NoiseLattice<const SIDE: usize> {
    scale: f32,
    values: [[f32; SIDE]; SIDE],
}

impl<const SIDE: usize> NoiseLattice<SIDE> {
    fn new(seed: u64, domain: u64, scale: f32) -> Self {
        let mut values = [[0.0; SIDE]; SIDE];
        for (iy, row) in values.iter_mut().enumerate() {
            for (ix, cell) in row.iter_mut().enumerate() {
                let raw = (noise(seed, domain, ix as i32, iy as i32) & 0xFFFF) as f32;
                *cell = raw / 32767.5 - 1.0;
            }
        }
        Self { scale, values }
    }

    #[inline]
    fn sample(&self, x: f32, y: f32) -> f32 {
        let gx = (x + 0.5).max(0.0) / self.scale;
        let gy = (y + 0.5).max(0.0) / self.scale;
        let ix = (gx.floor() as usize).min(SIDE - 2);
        let iy = (gy.floor() as usize).min(SIDE - 2);
        let fx = (gx - ix as f32).clamp(0.0, 1.0);
        let fy = (gy - iy as f32).clamp(0.0, 1.0);
        let sx = fx * fx * (3.0 - 2.0 * fx);
        let sy = fy * fy * (3.0 - 2.0 * fy);
        let v00 = self.values[iy][ix];
        let v10 = self.values[iy][ix + 1];
        let v01 = self.values[iy + 1][ix];
        let v11 = self.values[iy + 1][ix + 1];
        let a = v00 + (v10 - v00) * sx;
        let b = v01 + (v11 - v01) * sx;
        a + (b - a) * sy
    }
}

#[inline]
fn to_valley_vertex(wgx: i32, wgy: i32, meta: &ValleyMeta) -> (i32, i32) {
    let (ux, uy) = match meta.rotation {
        0 => (wgx, wgy),
        1 => (wgy, WORLD_SIZE - wgx),
        2 => (WORLD_SIZE - wgx, WORLD_SIZE - wgy),
        _ => (WORLD_SIZE - wgy, wgx),
    };
    let vgx = if meta.mirror { WORLD_SIZE - ux } else { ux };
    (vgx, uy)
}

#[inline]
fn tile_at_checked(tiles: &[Tile], tx: i32, ty: i32) -> Option<Tile> {
    ((0..WORLD_SIZE).contains(&tx) && (0..WORLD_SIZE).contains(&ty))
        .then(|| tiles[(ty * WORLD_SIZE + tx) as usize])
}

#[inline]
fn edge_is_road(tiles: &[Tile], x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    if x1 == x0 + 1 && y1 == y0 {
        tile_at_checked(tiles, x0, y0 - 1).is_some_and(|t| t.ground.is_road())
            || tile_at_checked(tiles, x0, y0).is_some_and(|t| t.ground.is_road())
    } else if x0 == x1 + 1 && y1 == y0 {
        tile_at_checked(tiles, x1, y0 - 1).is_some_and(|t| t.ground.is_road())
            || tile_at_checked(tiles, x1, y0).is_some_and(|t| t.ground.is_road())
    } else if y1 == y0 + 1 && x1 == x0 {
        tile_at_checked(tiles, x0 - 1, y0).is_some_and(|t| t.ground.is_road())
            || tile_at_checked(tiles, x0, y0).is_some_and(|t| t.ground.is_road())
    } else if y0 == y1 + 1 && x1 == x0 {
        tile_at_checked(tiles, x0 - 1, y1).is_some_and(|t| t.ground.is_road())
            || tile_at_checked(tiles, x0, y1).is_some_and(|t| t.ground.is_road())
    } else {
        false
    }
}

pub(super) fn generate(
    seed: u64,
    tiles: &[Tile],
    meta: &ValleyMeta,
    pads: &[PlannedPad],
) -> (TerrainGrid, Vec<f32>) {
    // Stage 1: Continuous Macro Topography
    let coarse = NoiseLattice::<10>::new(seed, 101, 64.0);
    let medium = NoiseLattice::<35>::new(seed, 102, 16.0);

    let river_phase = (noise(seed, 2, 0, 0) % 80) as i32;
    let mut river_knots = [0.0f32; 12];
    for (k, slot) in river_knots.iter_mut().enumerate() {
        *slot = ((noise(seed, 3, k as i32, 0) % 29) as i32 - 14) as f32;
    }
    let mut spine_knots = [0.0f32; 12];
    for (k, slot) in spine_knots.iter_mut().enumerate() {
        *slot = (noise(seed, 12, 0, k as i32) % 256) as f32;
    }

    let mut heights = vec![0.0f32; GRID_LEN].into_boxed_slice();
    let lake_rx = (meta.lake_radii.0 as f32).max(1.0);
    let lake_ry = (meta.lake_radii.1 as f32).max(1.0);
    let lake_r_min = lake_rx.min(lake_ry);

    for wgy in 0..=WORLD_SIZE {
        for wgx in 0..=WORLD_SIZE {
            let (vgx, vgy) = to_valley_vertex(wgx, wgy, meta);
            let vx = vgx as f32 - 0.5;
            let vy = vgy as f32 - 0.5;

            let y_p = (vy + river_phase as f32).max(0.0);
            let rk = ((y_p / 64.0).floor() as usize).min(10);
            let rt = (y_p / 64.0 - rk as f32).clamp(0.0, 1.0);
            let rst = rt * rt * (3.0 - 2.0 * rt);
            let channel_x = 355.0 + river_knots[rk] + (river_knots[rk + 1] - river_knots[rk]) * rst;
            let bank_dist = (vx - channel_x).abs();

            let ldx = (vx - meta.lake_center.0 as f32) / lake_rx;
            let ldy = (vy - meta.lake_center.1 as f32) / lake_ry;
            let lake_dist = ((ldx * ldx + ldy * ldy).sqrt() - 1.0).max(0.0) * lake_r_min;
            let water_bank_dist = bank_dist.min(lake_dist);

            let y_sp = vy.max(0.0);
            let sk = ((y_sp / 57.0).floor() as usize).min(10);
            let st = (y_sp / 57.0 - sk as f32).clamp(0.0, 1.0);
            let sst = st * st * (3.0 - 2.0 * st);
            let spine =
                25.0 + (spine_knots[sk] + (spine_knots[sk + 1] - spine_knots[sk]) * sst) / 5.0;
            let pass_factor = smoothstep(5.0, 37.0, (vy - meta.pass_y as f32).abs());
            let m_edge = spine * pass_factor;

            let town_x = smoothstep(
                meta.town_min.0 as f32 - 14.0,
                meta.town_min.0 as f32 + 6.0,
                vx,
            ) * smoothstep(
                meta.town_max.0 as f32 + 14.0,
                meta.town_max.0 as f32 - 6.0,
                vx,
            );
            let town_y = smoothstep(
                meta.town_min.1 as f32 - 14.0,
                meta.town_min.1 as f32 + 6.0,
                vy,
            ) * smoothstep(
                meta.town_max.1 as f32 + 14.0,
                meta.town_max.1 as f32 - 6.0,
                vy,
            );
            let town_mask = town_x * town_y;

            let floodplain = 0.50 + smoothstep(4.0, 50.0, water_bank_dist) * 2.30;
            let med = medium.sample(vx, vy);
            let hills = (coarse.sample(vx, vy) * 1.55 + med * 0.40)
                * smoothstep(6.0, 28.0, water_bank_dist)
                * (1.0 - 0.78 * town_mask);
            let foothill = smoothstep(0.0, 1.0, (m_edge + 36.0 - vx) / 36.0) * 3.8;
            let depth = (m_edge - vx).max(0.0);
            let mountain = depth * 0.52 + (med * 0.5 + 0.5) * smoothstep(0.0, 8.0, depth) * 4.2;

            heights[grid_idx(wgx, wgy)] = (floodplain + hills + foothill + mountain).max(0.45);
        }
    }

    // Vertex classification over incident tiles.
    let mut in_road_graph = vec![false; GRID_LEN];
    let mut touches_dry_road = vec![false; GRID_LEN];
    let mut touches_any_bridge = vec![false; GRID_LEN];
    let mut touches_lake = vec![false; GRID_LEN];
    let mut touches_mountain = vec![false; GRID_LEN];
    let mut water_count = vec![0u8; GRID_LEN];

    let mut road_id = vec![-1i32; GRID_LEN];
    let mut road_verts = Vec::with_capacity(16_384);

    for gy in 0..=WORLD_SIZE {
        for gx in 0..=WORLD_SIZE {
            let v = grid_idx(gx, gy);
            let mut road_any = false;
            let mut dry_road = false;
            let mut bridge_any = false;
            let mut lake_any = false;
            let mut mountain_any = false;
            let mut w_count = 0u8;

            for ty in (gy - 1).max(0)..=gy.min(WORLD_SIZE - 1) {
                for tx in (gx - 1).max(0)..=gx.min(WORLD_SIZE - 1) {
                    let t = tiles[(ty * WORLD_SIZE + tx) as usize];
                    if t.ground.is_road() {
                        road_any = true;
                        if t.water.is_none() {
                            dry_road = true;
                        }
                    }
                    if t.ground == Ground::Bridge {
                        bridge_any = true;
                    }
                    if t.ground == Ground::Mountain {
                        mountain_any = true;
                    }
                    if t.water.is_some() {
                        w_count += 1;
                        if t.water == Some(Water::Lake) {
                            lake_any = true;
                        }
                    }
                }
            }

            if road_any {
                in_road_graph[v] = true;
                road_id[v] = road_verts.len() as i32;
                road_verts.push((gx, gy, v));
            }
            touches_dry_road[v] = dry_road;
            touches_any_bridge[v] = bridge_any;
            touches_lake[v] = lake_any;
            touches_mountain[v] = mountain_any;
            water_count[v] = w_count;
        }
    }

    // Stage 2: Road Network Smoothing & Exact Two-Pass Dijkstra Grade Clamp
    let n_road = road_verts.len();
    let mut road_adj = vec![[u32::MAX; 4]; n_road];
    let mut road_z = Vec::with_capacity(n_road);
    let mut min_floor = Vec::with_capacity(n_road);

    for (ri, &(gx, gy, v)) in road_verts.iter().enumerate() {
        road_z.push(heights[v]);
        min_floor.push(if touches_any_bridge[v] {
            MIN_BRIDGE_CLEARANCE
        } else if touches_lake[v] {
            0.35
        } else {
            0.40
        });
        let mut slot = 0usize;
        for (nx, ny) in [(gx + 1, gy), (gx - 1, gy), (gx, gy + 1), (gx, gy - 1)] {
            if (0..=WORLD_SIZE).contains(&nx)
                && (0..=WORLD_SIZE).contains(&ny)
                && edge_is_road(tiles, gx, gy, nx, ny)
            {
                let nri = road_id[grid_idx(nx, ny)];
                debug_assert!(nri >= 0);
                road_adj[ri][slot] = nri as u32;
                slot += 1;
            }
        }
    }

    let mut next_road_z = vec![0.0f32; n_road];
    for _ in 0..5 {
        for ri in 0..n_road {
            let mut sum = road_z[ri];
            let mut count = 1.0f32;
            for &nb in &road_adj[ri] {
                if nb != u32::MAX {
                    sum += road_z[nb as usize];
                    count += 1.0;
                }
            }
            next_road_z[ri] = sum / count;
        }
        road_z.copy_from_slice(&next_road_z);
    }

    // Pass 2A: min-Dijkstra downward spike shave (`z[v] <= z[u] + L`).
    let mut min_heap = BinaryHeap::with_capacity(n_road);
    for (ri, &z) in road_z.iter().enumerate() {
        min_heap.push((Reverse(f32_to_ord_u32(z)), ri as u32));
    }
    while let Some((Reverse(bits), u_u32)) = min_heap.pop() {
        let u = u_u32 as usize;
        if bits != f32_to_ord_u32(road_z[u]) {
            continue;
        }
        let cand = road_z[u] + ROAD_LIPSCHITZ_STEP;
        for &nb in &road_adj[u] {
            if nb == u32::MAX {
                break;
            }
            let v = nb as usize;
            if cand < road_z[v] {
                road_z[v] = cand;
                min_heap.push((Reverse(f32_to_ord_u32(cand)), nb));
            }
        }
    }

    // Pass 2B: re-assert minimum bridge deck & road clearances.
    for (z, &floor) in road_z.iter_mut().zip(&min_floor) {
        if *z < floor {
            *z = floor;
        }
    }

    // Pass 2C: max-Dijkstra upward approach ramps (`z[v] >= z[u] - L`).
    let mut max_heap = BinaryHeap::with_capacity(n_road);
    for (ri, &z) in road_z.iter().enumerate() {
        max_heap.push((f32_to_ord_u32(z), ri as u32));
    }
    while let Some((bits, u_u32)) = max_heap.pop() {
        let u = u_u32 as usize;
        if bits != f32_to_ord_u32(road_z[u]) {
            continue;
        }
        let cand = road_z[u] - ROAD_LIPSCHITZ_STEP;
        for &nb in &road_adj[u] {
            if nb == u32::MAX {
                break;
            }
            let v = nb as usize;
            if cand > road_z[v] {
                road_z[v] = cand;
                max_heap.push((f32_to_ord_u32(cand), nb));
            }
        }
    }

    let mut bridges = HashMap::new();
    for ty in 0..WORLD_SIZE {
        for tx in 0..WORLD_SIZE {
            if tiles[(ty * WORLD_SIZE + tx) as usize].ground == Ground::Bridge {
                let z00 = road_z[road_id[grid_idx(tx, ty)] as usize];
                let z10 = road_z[road_id[grid_idx(tx + 1, ty)] as usize];
                let z01 = road_z[road_id[grid_idx(tx, ty + 1)] as usize];
                let z11 = road_z[road_id[grid_idx(tx + 1, ty + 1)] as usize];
                bridges.insert(Position::new(tx, ty), [z00, z10, z01, z11]);
            }
        }
    }

    let mut locked = vec![false; GRID_LEN];
    for (ri, &(_, _, v)) in road_verts.iter().enumerate() {
        if touches_dry_road[v] {
            heights[v] = road_z[ri];
            locked[v] = true;
        }
    }

    // Stage 3: Road-to-Terrain Pre-Blend & Flat Building Pads
    let mut band_dist = vec![u8::MAX; GRID_LEN];
    let mut bfs = VecDeque::with_capacity(16_384);
    for &(_, _, v) in &road_verts {
        if touches_dry_road[v] {
            band_dist[v] = 0;
            bfs.push_back(v);
        }
    }
    let mut band_verts = Vec::with_capacity(49_152);
    while let Some(v) = bfs.pop_front() {
        let d = band_dist[v];
        if d >= 8 {
            continue;
        }
        let gx = (v % TERRAIN_GRID_SIDE) as i32;
        let gy = (v / TERRAIN_GRID_SIDE) as i32;
        for (nx, ny) in [(gx + 1, gy), (gx - 1, gy), (gx, gy + 1), (gx, gy - 1)] {
            if (0..=WORLD_SIZE).contains(&nx) && (0..=WORLD_SIZE).contains(&ny) {
                let nv = grid_idx(nx, ny);
                if band_dist[nv] == u8::MAX && !locked[nv] && water_count[nv] == 0 {
                    band_dist[nv] = d + 1;
                    band_verts.push(nv);
                    bfs.push_back(nv);
                }
            }
        }
    }
    let mut scratch = vec![0.0f32; band_verts.len()];
    for _ in 0..4 {
        for (idx, &v) in band_verts.iter().enumerate() {
            let gx = (v % TERRAIN_GRID_SIDE) as i32;
            let gy = (v / TERRAIN_GRID_SIDE) as i32;
            let h_c = heights[v];
            let mut sum = h_c;
            for (nx, ny) in [(gx + 1, gy), (gx - 1, gy), (gx, gy + 1), (gx, gy - 1)] {
                let h_nb = if (0..=WORLD_SIZE).contains(&nx) && (0..=WORLD_SIZE).contains(&ny) {
                    let nv = grid_idx(nx, ny);
                    if water_count[nv] == 0 {
                        heights[nv]
                    } else {
                        h_c
                    }
                } else {
                    h_c
                };
                sum += h_nb;
            }
            scratch[idx] = sum * 0.20;
        }
        for (&v, &val) in band_verts.iter().zip(&scratch) {
            heights[v] = val;
        }
    }

    let mut foundations = Vec::with_capacity(pads.len());
    for pad in pads {
        let mut sum = 0.0f32;
        let mut count = 0usize;
        for gy in pad.min.y..=pad.max.y + 1 {
            for gx in pad.min.x..=pad.max.x + 1 {
                sum += heights[grid_idx(gx, gy)];
                count += 1;
            }
        }
        let z_avg = sum / count.max(1) as f32;

        // Sample nearest fronting street height around `pad.front`.
        let mut front_sum = 0.0f32;
        let mut front_count = 0usize;
        for gy in (pad.front.y - 1).max(0)..=(pad.front.y + 2).min(WORLD_SIZE) {
            for gx in (pad.front.x - 1).max(0)..=(pad.front.x + 2).min(WORLD_SIZE) {
                let v = grid_idx(gx, gy);
                if touches_dry_road[v] {
                    front_sum += heights[v];
                    front_count += 1;
                }
            }
        }
        let z_front = if front_count > 0 {
            front_sum / front_count as f32
        } else {
            let mut fsum = 0.0f32;
            for gy in pad.front.y..=pad.front.y + 1 {
                for gx in pad.front.x..=pad.front.x + 1 {
                    fsum += heights[grid_idx(gx, gy)];
                }
            }
            fsum * 0.25
        };

        let z_entry = 0.25
            * (heights[grid_idx(pad.entry.x, pad.entry.y)]
                + heights[grid_idx(pad.entry.x + 1, pad.entry.y)]
                + heights[grid_idx(pad.entry.x, pad.entry.y + 1)]
                + heights[grid_idx(pad.entry.x + 1, pad.entry.y + 1)]);

        let foundation_z = ((0.50 * z_front + 0.25 * z_entry + 0.25 * z_avg)
            .clamp(z_front - 0.35, z_front + 0.45)
            .max(0.50)
            * 64.0)
            .round()
            / 64.0;

        for gy in pad.min.y..=pad.max.y + 1 {
            for gx in pad.min.x..=pad.max.x + 1 {
                let v = grid_idx(gx, gy);
                heights[v] = foundation_z;
                locked[v] = true;
            }
        }
        foundations.push(foundation_z);
    }

    // Stage 4: Submerged Riverbeds & Lakebeds (`WATER_SURFACE_Z = 0.0`)
    let w_side = WORLD_SIZE as usize;
    let mut water_dist = vec![0u8; w_side * w_side];
    for (i, t) in tiles.iter().enumerate() {
        if t.water.is_some() {
            water_dist[i] = 8;
        }
    }
    for ty in 0..w_side {
        for tx in 0..w_side {
            let i = ty * w_side + tx;
            if water_dist[i] == 0 {
                continue;
            }
            let mut d = water_dist[i];
            if ty > 0 {
                d = d.min(water_dist[(ty - 1) * w_side + tx].saturating_add(1));
                if tx > 0 {
                    d = d.min(water_dist[(ty - 1) * w_side + tx - 1].saturating_add(1));
                }
                if tx + 1 < w_side {
                    d = d.min(water_dist[(ty - 1) * w_side + tx + 1].saturating_add(1));
                }
            } else {
                d = 1;
            }
            if tx > 0 {
                d = d.min(water_dist[ty * w_side + tx - 1].saturating_add(1));
            } else {
                d = 1;
            }
            water_dist[i] = d.min(8);
        }
    }
    for ty in (0..w_side).rev() {
        for tx in (0..w_side).rev() {
            let i = ty * w_side + tx;
            if water_dist[i] == 0 {
                continue;
            }
            let mut d = water_dist[i];
            if ty + 1 < w_side {
                d = d.min(water_dist[(ty + 1) * w_side + tx].saturating_add(1));
                if tx + 1 < w_side {
                    d = d.min(water_dist[(ty + 1) * w_side + tx + 1].saturating_add(1));
                }
                if tx > 0 {
                    d = d.min(water_dist[(ty + 1) * w_side + tx - 1].saturating_add(1));
                }
            } else {
                d = 1;
            }
            if tx + 1 < w_side {
                d = d.min(water_dist[ty * w_side + tx + 1].saturating_add(1));
            } else {
                d = 1;
            }
            water_dist[i] = d.min(8);
        }
    }

    for gy in 0..=WORLD_SIZE {
        for gx in 0..=WORLD_SIZE {
            let v = grid_idx(gx, gy);
            if locked[v] || water_count[v] == 0 {
                continue;
            }
            let mut near_dry_road = false;
            for ny in (gy - 1).max(0)..=(gy + 1).min(WORLD_SIZE) {
                for nx in (gx - 1).max(0)..=(gx + 1).min(WORLD_SIZE) {
                    if touches_dry_road[grid_idx(nx, ny)] {
                        near_dry_road = true;
                        break;
                    }
                }
                if near_dry_road {
                    break;
                }
            }
            let mut bed = if water_count[v] <= 3 {
                WATER_SURFACE_Z - 0.16
            } else {
                let mut min_d = 8u8;
                for ty in (gy - 1).max(0)..=gy.min(WORLD_SIZE - 1) {
                    for tx in (gx - 1).max(0)..=gx.min(WORLD_SIZE - 1) {
                        let d = water_dist[(ty * WORLD_SIZE + tx) as usize];
                        if d > 0 {
                            min_d = min_d.min(d);
                        }
                    }
                }
                WATER_SURFACE_Z - (0.60 + 0.26 * (min_d as f32))
            };
            if touches_any_bridge[v] || near_dry_road {
                bed = bed.min(WATER_SURFACE_Z - 1.25);
            }
            heights[v] = bed;
            locked[v] = true;
        }
    }

    // Guarantee every water tile center (`0.5 * (z00 + z11)`) remains strictly submerged beneath `WATER_SURFACE_Z`.
    for ty in 0..WORLD_SIZE {
        for tx in 0..WORLD_SIZE {
            let t = tiles[(ty * WORLD_SIZE + tx) as usize];
            if t.water.is_none() {
                continue;
            }
            let v00 = grid_idx(tx, ty);
            let v11 = grid_idx(tx + 1, ty + 1);
            let z00 = heights[v00];
            let z11 = heights[v11];
            if 0.5 * (z00 + z11) > WATER_SURFACE_Z - 0.15 {
                let floor = if t.ground == Ground::Bridge {
                    WATER_SURFACE_Z - 1.25
                } else {
                    WATER_SURFACE_Z - 0.15
                };
                if !touches_dry_road[v11] {
                    heights[v11] = heights[v11]
                        .min(2.0 * (WATER_SURFACE_Z - 0.15) - z00)
                        .min(floor);
                    locked[v11] = true;
                } else if !touches_dry_road[v00] {
                    heights[v00] = heights[v00]
                        .min(2.0 * (WATER_SURFACE_Z - 0.15) - z11)
                        .min(floor);
                    locked[v00] = true;
                }
            }
        }
    }

    // Stage 5: Apron Relaxation Around Locked Features
    let mut apron_dist = vec![u8::MAX; GRID_LEN];
    bfs.clear();
    for (v, &is_locked) in locked.iter().enumerate() {
        if is_locked {
            apron_dist[v] = 0;
            bfs.push_back(v);
        }
    }
    let mut apron = Vec::with_capacity(65_536);
    while let Some(v) = bfs.pop_front() {
        let d = apron_dist[v];
        if d >= 6 {
            continue;
        }
        let gx = (v % TERRAIN_GRID_SIDE) as i32;
        let gy = (v / TERRAIN_GRID_SIDE) as i32;
        for ny in (gy - 1).max(0)..=(gy + 1).min(WORLD_SIZE) {
            for nx in (gx - 1).max(0)..=(gx + 1).min(WORLD_SIZE) {
                let nv = grid_idx(nx, ny);
                if apron_dist[nv] == u8::MAX && !locked[nv] && !touches_mountain[nv] {
                    apron_dist[nv] = d + 1;
                    apron.push(nv);
                    bfs.push_back(nv);
                }
            }
        }
    }

    let mut apron_scratch = vec![0.0f32; apron.len()];
    for _ in 0..6 {
        for (idx, &v) in apron.iter().enumerate() {
            let gx = (v % TERRAIN_GRID_SIDE) as i32;
            let gy = (v / TERRAIN_GRID_SIDE) as i32;
            let h_c = heights[v];
            let h_w = if gx > 0 { heights[v - 1] } else { h_c };
            let h_e = if gx < WORLD_SIZE { heights[v + 1] } else { h_c };
            let h_n = if gy > 0 {
                heights[v - TERRAIN_GRID_SIDE]
            } else {
                h_c
            };
            let h_s = if gy < WORLD_SIZE {
                heights[v + TERRAIN_GRID_SIDE]
            } else {
                h_c
            };
            apron_scratch[idx] = (0.20 * h_c + 0.20 * (h_w + h_e + h_n + h_s)).max(0.35);
        }
        for (&v, &val) in apron.iter().zip(&apron_scratch) {
            heights[v] = val;
        }
    }

    (TerrainGrid::new(heights, bridges), foundations)
}
