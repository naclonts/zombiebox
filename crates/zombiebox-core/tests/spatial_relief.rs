use zombiebox_core::{
    ActionGame, CHUNK_SIZE, CHUNKS_PER_SIDE, Ground, MIN_BRIDGE_CLEARANCE, Position,
    TerrainTriangle, Vec2, Vec3, WATER_SURFACE_Z, WORLD_SIZE, World,
};

const SEEDS: [u64; 8] = [0, 1, 7, 42, 641, 4567, 93513, u64::MAX];

#[test]
fn boundary_precision_and_out_of_bounds_queries() {
    let world = World::new(42);
    let max = WORLD_SIZE as f32 - 0.5;

    let min_corner = world.terrain_at(Vec2::new(-0.5, -0.5)).expect("min corner");
    assert_eq!(min_corner.tile, Position::new(0, 0));

    let max_corner = world.terrain_at(Vec2::new(max, max)).expect("max corner");
    assert_eq!(
        max_corner.tile,
        Position::new(WORLD_SIZE - 1, WORLD_SIZE - 1)
    );

    for bad in [
        Vec2::new(-0.5001, 0.0),
        Vec2::new(0.0, -0.5001),
        Vec2::new(max + 0.0001, 0.0),
        Vec2::new(0.0, max + 0.0001),
        Vec2::new(f32::NAN, 10.0),
        Vec2::new(10.0, f32::NAN),
        Vec2::new(f32::INFINITY, 10.0),
        Vec2::new(f32::NEG_INFINITY, 10.0),
    ] {
        assert!(world.terrain_at(bad).is_none(), "expected None for {bad:?}");
        assert!(world.elevation_at(bad).is_none());
        assert!(world.normal_at(bad).is_none());
    }

    assert!(world.grid_height(-1, 0).is_none());
    assert!(world.grid_height(WORLD_SIZE + 1, 0).is_none());
    assert!(world.grid_height(WORLD_SIZE, WORLD_SIZE).is_some());
    assert!(world.chunk_heights(-1, 0).is_none());
    assert!(world.chunk_heights(CHUNKS_PER_SIDE, 0).is_none());
}

#[test]
fn birch_street_flat_fixture_invariants() {
    let world = World::birch_street(42);
    assert!(world.is_flat());

    for gx in [0, 1, 16, 256, 512] {
        for gy in [0, 1, 16, 256, 512] {
            assert_eq!(world.grid_height(gx, gy), Some(0.0));
            assert_eq!(world.grid_normal(gx, gy), Some(Vec3::Z));
        }
    }

    for &(x, y) in &[
        (-0.5, -0.5),
        (0.0, 0.0),
        (8.25, 8.75),
        (16.0, 16.0),
        (511.5, 511.5),
    ] {
        let s = world.terrain_at(Vec2::new(x, y)).unwrap();
        assert_eq!(s.z, 0.0);
        assert_eq!(s.normal, Vec3::Z);
        assert_eq!(s.water_z, None);
        assert_eq!(s.slope(), 0.0);
        assert!(!s.is_submerged());
        assert!(world.bridge_deck_at(Vec2::new(x, y)).is_none());
    }

    let game = ActionGame::residential(42);
    assert!(!game.buildings().is_empty());
    for b in game.buildings() {
        assert_eq!(b.foundation_z, 0.0);
    }
}

#[test]
fn seed_determinism_uniqueness_and_visit_order_independence() {
    let a = World::new(42);
    let b = World::new(42);
    assert!(!a.is_flat());

    for cy in (0..CHUNKS_PER_SIDE).rev() {
        for cx in (0..CHUNKS_PER_SIDE).rev() {
            assert_eq!(a.chunk_heights(cx, cy), b.chunk_heights(cx, cy));
            assert_eq!(a.chunk_normals(cx, cy), b.chunk_normals(cx, cy));
        }
    }

    let sites_a = a.sites().unwrap();
    let sites_b = b.sites().unwrap();
    assert_eq!(sites_a.buildings.len(), sites_b.buildings.len());
    for (ba, bb) in sites_a.buildings.iter().zip(&sites_b.buildings) {
        assert_eq!(ba.foundation_z, bb.foundation_z);
    }

    for y in 0..WORLD_SIZE {
        for x in 0..WORLD_SIZE {
            let p = Position::new(x, y);
            assert_eq!(a.bridge_quad(p), b.bridge_quad(p));
        }
    }

    let c = World::new((1 << 42) + 42);
    assert_ne!(a.chunk_heights(10, 10), c.chunk_heights(10, 10));
    let foundations_a: Vec<f32> = sites_a.buildings.iter().map(|x| x.foundation_z).collect();
    let foundations_c: Vec<f32> = c
        .sites()
        .unwrap()
        .buildings
        .iter()
        .map(|x| x.foundation_z)
        .collect();
    assert_ne!(foundations_a, foundations_c);
}

#[test]
fn chunk_edge_position_and_normal_continuity() {
    for seed in [0u64, 42, u64::MAX] {
        let world = World::new(seed);
        for cy in 0..CHUNKS_PER_SIDE {
            for cx in 0..CHUNKS_PER_SIDE {
                let h = world.chunk_heights(cx, cy).unwrap();
                let n = world.chunk_normals(cx, cy).unwrap();
                if cx + 1 < CHUNKS_PER_SIDE {
                    let hr = world.chunk_heights(cx + 1, cy).unwrap();
                    let nr = world.chunk_normals(cx + 1, cy).unwrap();
                    for ly in 0..h.len() {
                        assert_eq!(h[ly][16], hr[ly][0], "seed {seed} east height seam");
                        assert_eq!(n[ly][16], nr[ly][0], "seed {seed} east normal seam");
                    }
                    let seam_x = ((cx + 1) * CHUNK_SIZE) as f32 - 0.5;
                    for i in 0..64 {
                        let y = (cy * CHUNK_SIZE) as f32 - 0.45 + (i as f32) * (15.9 / 63.0);
                        let xa = seam_x - 1e-4;
                        let xb = seam_x + 1e-4;
                        let sa = world.terrain_at(Vec2::new(xa, y)).unwrap();
                        let sb = world.terrain_at(Vec2::new(xb, y)).unwrap();
                        let ss = world.terrain_at(Vec2::new(seam_x, y)).unwrap();
                        let za_proj = sa.z + (-sa.normal.x / sa.normal.z) * (seam_x - xa);
                        let zb_proj = sb.z + (-sb.normal.x / sb.normal.z) * (seam_x - xb);
                        assert!(
                            (za_proj - ss.z).abs() < 1e-4 && (zb_proj - ss.z).abs() < 1e-4,
                            "seed {seed} east crack at ({seam_x}, {y}): {za_proj} / {} / {zb_proj}",
                            ss.z
                        );
                        let slope_bound = 5e-4 + 3e-4 * sa.slope().max(sb.slope());
                        assert!(
                            (sa.z - sb.z).abs() <= slope_bound,
                            "seed {seed} east jump at ({seam_x}, {y}): {} vs {}",
                            sa.z,
                            sb.z
                        );
                    }
                }
                if cy + 1 < CHUNKS_PER_SIDE {
                    let hd = world.chunk_heights(cx, cy + 1).unwrap();
                    let nd = world.chunk_normals(cx, cy + 1).unwrap();
                    for lx in 0..h[0].len() {
                        assert_eq!(h[16][lx], hd[0][lx], "seed {seed} south height seam");
                        assert_eq!(n[16][lx], nd[0][lx], "seed {seed} south normal seam");
                    }
                    let seam_y = ((cy + 1) * CHUNK_SIZE) as f32 - 0.5;
                    for i in 0..64 {
                        let x = (cx * CHUNK_SIZE) as f32 - 0.45 + (i as f32) * (15.9 / 63.0);
                        let ya = seam_y - 1e-4;
                        let yb = seam_y + 1e-4;
                        let sa = world.terrain_at(Vec2::new(x, ya)).unwrap();
                        let sb = world.terrain_at(Vec2::new(x, yb)).unwrap();
                        let ss = world.terrain_at(Vec2::new(x, seam_y)).unwrap();
                        let za_proj = sa.z + (-sa.normal.y / sa.normal.z) * (seam_y - ya);
                        let zb_proj = sb.z + (-sb.normal.y / sb.normal.z) * (seam_y - yb);
                        assert!(
                            (za_proj - ss.z).abs() < 1e-4 && (zb_proj - ss.z).abs() < 1e-4,
                            "seed {seed} south crack at ({x}, {seam_y}): {za_proj} / {} / {zb_proj}",
                            ss.z
                        );
                        let slope_bound = 5e-4 + 3e-4 * sa.slope().max(sb.slope());
                        assert!(
                            (sa.z - sb.z).abs() <= slope_bound,
                            "seed {seed} south jump at ({x}, {seam_y}): {} vs {}",
                            sa.z,
                            sb.z
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn flat_building_pads_and_action_building_info() {
    for seed in SEEDS {
        let world = World::new(seed);
        let sites = world.sites().unwrap();
        assert!(!sites.buildings.is_empty());
        for b in &sites.buildings {
            assert!(
                b.foundation_z >= 0.50,
                "seed {seed} {:?} low foundation {}",
                b.kind,
                b.foundation_z
            );
            for gy in b.min.y..=b.max.y + 1 {
                for gx in b.min.x..=b.max.x + 1 {
                    assert_eq!(
                        world.grid_height(gx, gy),
                        Some(b.foundation_z),
                        "seed {seed} vertex ({gx},{gy}) mismatch on {:?}",
                        b.kind
                    );
                }
            }
            for ty in b.min.y..=b.max.y {
                for tx in b.min.x..=b.max.x {
                    for oy in [-0.45f32, 0.0, 0.45] {
                        for ox in [-0.45f32, 0.0, 0.45] {
                            let s = world
                                .terrain_at(Vec2::new(tx as f32 + ox, ty as f32 + oy))
                                .unwrap();
                            assert_eq!(s.z, b.foundation_z);
                            assert_eq!(s.normal, Vec3::Z);
                        }
                    }
                }
            }
        }
    }

    let world42 = World::new(42);
    let game42 = ActionGame::county(42);
    assert!(!game42.buildings().is_empty());
    for info in game42.buildings() {
        assert!(info.foundation_z >= 0.50);
        let cx = info.x as f32 + (info.width as f32 - 1.0) * 0.5;
        let cy = info.y as f32 + (info.height as f32 - 1.0) * 0.5;
        let s = world42.terrain_at(Vec2::new(cx, cy)).unwrap();
        assert_eq!(s.z, info.foundation_z);
    }
}

#[test]
fn road_grade_lipschitz_limit_and_bridge_deck_compatibility() {
    for seed in SEEDS {
        let world = World::new(seed);
        for ty in 0..WORLD_SIZE {
            for tx in 0..WORLD_SIZE {
                let p = Position::new(tx, ty);
                let tile = world.tile_at(p).unwrap();
                if tile.ground.is_road() && tile.water.is_none() {
                    let s_lr = world
                        .terrain_at(Vec2::new(tx as f32 + 0.25, ty as f32 - 0.25))
                        .unwrap();
                    let s_ul = world
                        .terrain_at(Vec2::new(tx as f32 - 0.25, ty as f32 + 0.25))
                        .unwrap();
                    assert_eq!(s_lr.triangle, TerrainTriangle::LowerRight);
                    assert_eq!(s_ul.triangle, TerrainTriangle::UpperLeft);
                    assert!(
                        s_lr.slope() <= 0.20 + 1e-5,
                        "seed {seed} road LR slope {} at {p:?}",
                        s_lr.slope()
                    );
                    assert!(
                        s_ul.slope() <= 0.20 + 1e-5,
                        "seed {seed} road UL slope {} at {p:?}",
                        s_ul.slope()
                    );

                    let p_z = world.elevation_at(Vec2::new(tx as f32, ty as f32)).unwrap();
                    for (dx, dy) in [(1, 0), (0, 1)] {
                        let q = Position::new(tx + dx, ty + dy);
                        if let Some(qt) = world.tile_at(q)
                            && qt.ground.is_road()
                            && qt.water.is_none()
                        {
                            let q_z = world
                                .elevation_at(Vec2::new(q.x as f32, q.y as f32))
                                .unwrap();
                            assert!(
                                (p_z - q_z).abs() <= 0.20 + 1e-5,
                                "seed {seed} road step {} between {p:?} and {q:?}",
                                (p_z - q_z).abs()
                            );
                        }
                    }
                }

                if tile.ground == Ground::Bridge {
                    let center = Vec2::new(tx as f32, ty as f32);
                    let deck = world.bridge_deck_at(center).expect("bridge deck sample");
                    assert!(
                        deck.z >= WATER_SURFACE_Z + MIN_BRIDGE_CLEARANCE - 1e-5,
                        "seed {seed} low bridge deck {} at {p:?}",
                        deck.z
                    );
                    assert!(
                        deck.slope() <= 0.20 + 1e-5,
                        "seed {seed} steep bridge deck {} at {p:?}",
                        deck.slope()
                    );

                    let bq = world.bridge_quad(p).unwrap();
                    let checks = [
                        (Position::new(tx - 1, ty), [0, 2], [1, 3]),
                        (Position::new(tx + 1, ty), [1, 3], [0, 2]),
                        (Position::new(tx, ty - 1), [0, 1], [2, 3]),
                        (Position::new(tx, ty + 1), [2, 3], [0, 1]),
                    ];
                    for (q, bi, qi) in checks {
                        if let Some(qt) = world.tile_at(q)
                            && qt.ground.is_road()
                            && qt.water.is_none()
                        {
                            let tq = world.terrain_quad(q).unwrap();
                            assert_eq!(
                                bq[bi[0]], tq[qi[0]],
                                "seed {seed} abutment seam 0 at {p:?}"
                            );
                            assert_eq!(
                                bq[bi[1]], tq[qi[1]],
                                "seed {seed} abutment seam 1 at {p:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn submerged_waterbeds_and_plausible_relief() {
    for seed in SEEDS {
        let world = World::new(seed);
        let wake = world.wake_position();
        let wake_z = world
            .elevation_at(Vec2::new(wake.x as f32, wake.y as f32))
            .unwrap();

        let mut max_mountain_z = f32::NEG_INFINITY;
        let mut min_under_bridge_z = f32::INFINITY;
        let mut bridge_count = 0usize;

        for ty in 0..WORLD_SIZE {
            for tx in 0..WORLD_SIZE {
                let p = Position::new(tx, ty);
                let tile = world.tile_at(p).unwrap();
                let center = Vec2::new(tx as f32, ty as f32);
                let s = world.terrain_at(center).unwrap();

                if matches!(tile.ground, Ground::River | Ground::Lake) {
                    assert_eq!(s.water_z, Some(WATER_SURFACE_Z));
                    assert!(
                        s.is_submerged(),
                        "seed {seed} water tile {p:?} ({:?}) not submerged: z={}",
                        tile.ground,
                        s.z
                    );
                }

                if tile.ground == Ground::Bridge {
                    bridge_count += 1;
                    let q = world.terrain_quad(p).unwrap();
                    let touches_both_diag_dry_road =
                        q[0] >= WATER_SURFACE_Z && q[3] >= WATER_SURFACE_Z;
                    if !touches_both_diag_dry_road {
                        assert!(
                            s.is_submerged(),
                            "seed {seed} bridge riverbed {p:?} not submerged: z={}, quad={q:?}",
                            s.z
                        );
                    }
                    assert!(
                        q.iter().any(|&v| v <= WATER_SURFACE_Z - 1.0),
                        "seed {seed} bridge {p:?} lacks submerged riverbed corner: quad={q:?}"
                    );
                    for v in q {
                        min_under_bridge_z = min_under_bridge_z.min(v);
                    }
                }

                if tile.ground == Ground::Mountain {
                    max_mountain_z = max_mountain_z.max(s.z);
                }
            }
        }

        assert!(bridge_count > 0, "seed {seed} expected bridges");
        assert!(
            min_under_bridge_z <= WATER_SURFACE_Z - 1.0,
            "seed {seed} min under-bridge riverbed {min_under_bridge_z}"
        );
        assert!(
            max_mountain_z >= wake_z + 10.0,
            "seed {seed} mountain peak {max_mountain_z} vs wake {wake_z}"
        );
    }
}
