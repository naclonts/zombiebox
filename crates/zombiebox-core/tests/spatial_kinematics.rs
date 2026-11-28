use std::collections::HashMap;
use zombiebox_core::{
    Aabb3, ActionGame, BodyShape, BodyState, Collider, ColliderKind, DT, DamageKind, FLOOR_H,
    Ground, HitSurface, Material, MotionState, MoveParams, Position, QueryFilter, Ray3,
    SpatialWorld, StairGeometry, SupportId, SupportSurface, Surface, SurfaceQuad, TerrainSample,
    TerrainTriangle, Tile3D, TileKind, Vec2, Vec3, WALL_HALF_THICKNESS, WATER_SURFACE_Z,
    WORLD_SIZE, floor_elevation, move_kinematic_with_horizontal_vel, select_grounded_support,
    step_kinematic_body, sweep_airborne_support,
};

#[derive(Default)]
#[allow(clippy::type_complexity)]
struct SyntheticWorld {
    supports: Vec<SupportSurface>,
    colliders: Vec<Collider>,
    quads: Vec<SurfaceQuad>,
    heights: HashMap<Position, [f32; 4]>,
    terrain_fn: Option<Box<dyn Fn(Vec2) -> Option<TerrainSample>>>,
    support_fn: Option<Box<dyn Fn(Vec2, &mut Vec<SupportSurface>)>>,
}

impl SpatialWorld for SyntheticWorld {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
        if let Some(f) = &self.terrain_fn {
            return f(xy);
        }
        let tx = zombiebox_core::world_to_tile_coord(xy.x);
        let ty = zombiebox_core::world_to_tile_coord(xy.y);
        let tile = Position::new(tx, ty);
        if let Some(&corners) = self.heights.get(&tile) {
            let u = (xy.x - (tx as f32 - 0.5)).clamp(0.0, 1.0);
            let v = (xy.y - (ty as f32 - 0.5)).clamp(0.0, 1.0);
            let (z, normal, triangle) = zombiebox_core::sample_quad_00_11(
                corners[0], corners[1], corners[2], corners[3], u, v,
            );
            return Some(TerrainSample {
                z,
                normal,
                tile,
                water_z: None,
                triangle,
            });
        }
        Some(TerrainSample {
            z: 0.0,
            normal: Vec3::Z,
            tile,
            water_z: None,
            triangle: TerrainTriangle::LowerRight,
        })
    }

    fn supports_at(&self, xy: Vec2, out: &mut Vec<SupportSurface>) {
        out.clear();
        if let Some(f) = &self.support_fn {
            f(xy, out);
            return;
        }
        if !self.supports.is_empty() {
            out.extend_from_slice(&self.supports);
            return;
        }
        if let Some(t) = self.terrain_at(xy) {
            out.push(SupportSurface {
                id: SupportId::Terrain,
                z: t.z,
                normal: t.normal,
                walkable: true,
                is_stair: false,
                floor: 0,
                surface: Surface::Grass,
                water_z: t.water_z,
            });
        }
    }

    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
        out.clear();
        for &c in &self.colliders {
            if c.bounds.intersects(bounds) {
                out.push(c);
            }
        }
    }

    fn surface_quads_in(&self, min_tile: Position, max_tile: Position, out: &mut Vec<SurfaceQuad>) {
        out.clear();
        for &q in &self.quads {
            out.push(q);
        }
        if self.quads.is_empty() {
            for ty in min_tile.y..=max_tile.y {
                for tx in min_tile.x..=max_tile.x {
                    let p = Position::new(tx, ty);
                    let x0 = tx as f32 - 0.5;
                    let y0 = ty as f32 - 0.5;
                    let x1 = tx as f32 + 0.5;
                    let y1 = ty as f32 + 0.5;
                    if let Some(&corners) = self.heights.get(&p) {
                        out.push(SurfaceQuad {
                            surface: HitSurface::Terrain { tile: p },
                            x0,
                            y0,
                            x1,
                            y1,
                            corners,
                            material: Material::None,
                        });
                    }
                }
            }
        }
    }
}

fn flat_support(id: SupportId, z: f32, floor: u8, surface: Surface) -> SupportSurface {
    SupportSurface {
        id,
        z,
        normal: Vec3::Z,
        walkable: surface != Surface::Water && surface != Surface::Mountain,
        is_stair: matches!(id, SupportId::Stair(_)),
        floor,
        surface,
        water_z: (surface == Surface::Water).then_some(WATER_SURFACE_Z),
    }
}

#[test]
fn support_selection_anti_snap_and_airborne_sweep() {
    let params = MoveParams::default();
    let bridge_pos = Position::new(10, 10);
    let slab_tile = Tile3D::new(10, 10, 1);

    let candidates = [
        flat_support(SupportId::Terrain, -1.30, 0, Surface::Water),
        flat_support(SupportId::Terrain, 0.40, 0, Surface::Grass),
        flat_support(SupportId::Bridge(bridge_pos), 0.95, 0, Surface::Bridge),
        flat_support(SupportId::Floor(slab_tile), FLOOR_H, 1, Surface::WoodFloor),
    ];

    // Grounded on bank at z = 0.40 must never snap upward onto bridge (+0.95) or 2F slab (+1.388).
    let bank = select_grounded_support(
        &candidates,
        0.40,
        Some(SupportId::Terrain),
        params.max_step_up,
        params.max_step_down,
        params.max_walkable_slope,
        true,
    )
    .expect("bank support");
    assert_eq!(bank.id, SupportId::Terrain);
    assert!((bank.z - 0.40).abs() < 1e-6);

    // Grounded near bridge deck elevation (0.92) selects the bridge deck (+0.95, dz = +0.03 <= 0.22).
    let bridge = select_grounded_support(
        &candidates,
        0.92,
        Some(SupportId::Terrain),
        params.max_step_up,
        params.max_step_down,
        params.max_walkable_slope,
        true,
    )
    .expect("bridge support");
    assert_eq!(bridge.id, SupportId::Bridge(bridge_pos));
    assert!((bridge.z - 0.95).abs() < 1e-6);

    // Grounded at 1.388 stepping onto air where only z = 0.0 exists returns None -> Airborne.
    let ground_only = [flat_support(SupportId::Terrain, 0.0, 0, Surface::Grass)];
    assert!(
        select_grounded_support(
            &ground_only,
            FLOOR_H,
            Some(SupportId::Floor(slab_tile)),
            params.max_step_up,
            params.max_step_down,
            params.max_walkable_slope,
            true,
        )
        .is_none()
    );

    // Riverbed wading with avoid_water == false is allowed, while Mountain is always rejected.
    let river_and_mtn = [
        flat_support(SupportId::Terrain, -1.30, 0, Surface::Water),
        flat_support(SupportId::Terrain, 0.0, 0, Surface::Mountain),
    ];
    let submerged = select_grounded_support(
        &river_and_mtn,
        -1.30,
        Some(SupportId::Terrain),
        params.max_step_up,
        params.max_step_down,
        params.max_walkable_slope,
        false,
    )
    .expect("submerged riverbed support allowed when avoid_water == false");
    assert!((submerged.z - (-1.30)).abs() < 1e-6);

    // Airborne sweep selects highest surface in [z_to, z_from].
    let hit = sweep_airborne_support(
        &candidates,
        FLOOR_H + 0.12,
        0.80,
        params.max_walkable_slope,
        true,
    )
    .expect("airborne landing on 2F slab");
    assert!(hit.can_land);
    assert_eq!(hit.surface.id, SupportId::Floor(slab_tile));
}

#[test]
fn stair_geometry_sample_matches_surface_quads() {
    let tile = Tile3D::new(4, 5, 0);
    for (low_dir, high_dir) in [((-1, 0), (1, 0)), ((-1, 0), (0, -1)), ((1, 0), (0, 1))] {
        let stair = StairGeometry::new(tile, 1.25, low_dir, high_dir);
        let mut world = SyntheticWorld::default();
        stair.push_quads(Material::WoodLight, &mut world.quads);

        for iy in 1..10 {
            for ix in 1..10 {
                let x = tile.x as f32 - 0.5 + ix as f32 * 0.1;
                let y = tile.y as f32 - 0.5 + iy as f32 * 0.1;
                let (sz, sn) = stair.sample(Vec2::new(x, y));
                let ray = Ray3::new(Vec3::new(x, y, 5.0), Vec3::new(0.0, 0.0, -1.0));
                let hit = world.raycast(ray, 10.0).expect("ray hits stair quad");
                assert!((hit.point.z - sz).abs() < 1e-5);
                // Strictly inside each sub-quad triangle (away from the `00 -> 11` diagonal
                // and `0.5` sub-quad crease), normals match exactly.
                if ix % 5 != 0 && iy % 5 != 0 && (ix % 5) != (iy % 5) {
                    assert!((hit.normal.dot(sn) - 1.0).abs() < 1e-4);
                }
            }
        }
    }
}

#[test]
fn synthetic_ramps_curbs_steep_cliffs_and_ceiling_bonk() {
    let shape = BodyShape::new(0.23, 1.05);
    let params = MoveParams::default();

    // 1. Smooth 15% ramp (slope = 0.15 <= 0.85): climbs & descends with zero airborne flicker.
    let ramp_world = SyntheticWorld {
        support_fn: Some(Box::new(|xy, out| {
            let z = (xy.x.max(0.0)) * 0.15;
            let n = if xy.x >= 0.0 {
                Vec3::new(-0.15, 0.0, 1.0).normalized()
            } else {
                Vec3::Z
            };
            out.push(SupportSurface {
                id: SupportId::Terrain,
                z,
                normal: n,
                walkable: true,
                is_stair: false,
                floor: 0,
                surface: Surface::Road,
                water_z: None,
            });
        })),
        ..Default::default()
    };

    let mut state = BodyState::grounded(Vec3::new(-0.5, 0.0, 0.0), 0.0, SupportId::Terrain);
    for _ in 0..90 {
        let res = move_kinematic_with_horizontal_vel(
            &ramp_world,
            shape,
            state,
            Vec2::new(4.0, 0.0),
            DT,
            &params,
        );
        state = res.state;
        assert!(
            state.motion.is_grounded(),
            "must stay grounded ascending 15% ramp"
        );
    }
    assert!(state.position.z > 0.70);
    for _ in 0..90 {
        let res = move_kinematic_with_horizontal_vel(
            &ramp_world,
            shape,
            state,
            Vec2::new(-4.0, 0.0),
            DT,
            &params,
        );
        state = res.state;
        assert!(
            state.motion.is_grounded(),
            "must stay grounded descending 15% ramp"
        );
    }
    assert!(state.position.z.abs() < 1e-4);

    // 2. Discrete 0.18 curb climbs cleanly (stepped == true); 0.38 sill blocks horizontally.
    let curb_world = SyntheticWorld {
        support_fn: Some(Box::new(|xy, out| {
            let z = if xy.x >= 1.0 { 0.18 } else { 0.0 };
            out.push(flat_support(SupportId::Terrain, z, 0, Surface::Sidewalk));
        })),
        ..Default::default()
    };
    let mut curb_state = BodyState::grounded(Vec3::new(0.5, 0.0, 0.0), 0.0, SupportId::Terrain);
    let mut saw_step = false;
    for _ in 0..30 {
        let res = move_kinematic_with_horizontal_vel(
            &curb_world,
            shape,
            curb_state,
            Vec2::new(3.0, 0.0),
            DT,
            &params,
        );
        saw_step |= res.stepped;
        curb_state = res.state;
    }
    assert!(saw_step, "0.18 curb must report stepped == true");
    assert!((curb_state.position.z - 0.18).abs() < 1e-4);

    let sill_world = SyntheticWorld {
        support_fn: Some(Box::new(|xy, out| {
            let z = if xy.x >= 1.0 { 0.38 } else { 0.0 };
            out.push(flat_support(SupportId::Terrain, z, 0, Surface::Sidewalk));
        })),
        ..Default::default()
    };
    let mut sill_state = BodyState::grounded(Vec3::new(0.5, 0.0, 0.0), 0.0, SupportId::Terrain);
    let mut ever_blocked = false;
    for _ in 0..30 {
        let res = move_kinematic_with_horizontal_vel(
            &sill_world,
            shape,
            sill_state,
            Vec2::new(3.0, 0.0),
            DT,
            &params,
        );
        ever_blocked |= res.blocked_xy;
        sill_state = res.state;
    }
    assert!(ever_blocked, "0.38 sill (> 0.22) must block movement");
    assert!(sill_state.position.x < 1.0);

    // 3. Unwalkable 60-deg cliff (slope = 1.732 > 0.85): blocks grounded walk-up and slides airborne drop.
    let cliff_slope = 3.0_f32.sqrt(); // tan(60 deg) ≈ 1.732
    let cliff_world = SyntheticWorld {
        support_fn: Some(Box::new(move |xy, out| {
            if xy.x <= 1.0 {
                out.push(flat_support(SupportId::Terrain, 0.0, 0, Surface::Grass));
            } else {
                let z = (xy.x - 1.0) * cliff_slope;
                let n = Vec3::new(-cliff_slope, 0.0, 1.0).normalized();
                out.push(SupportSurface {
                    id: SupportId::Terrain,
                    z,
                    normal: n,
                    walkable: true,
                    is_stair: false,
                    floor: 0,
                    surface: Surface::Grass,
                    water_z: None,
                });
            }
        })),
        ..Default::default()
    };

    let mut base_state = BodyState::grounded(Vec3::new(0.6, 0.0, 0.0), 0.0, SupportId::Terrain);
    let mut cliff_blocked = false;
    for _ in 0..30 {
        let res = move_kinematic_with_horizontal_vel(
            &cliff_world,
            shape,
            base_state,
            Vec2::new(3.0, 0.0),
            DT,
            &params,
        );
        cliff_blocked |= res.blocked_xy;
        base_state = res.state;
    }
    assert!(cliff_blocked);
    assert!(base_state.position.x <= 1.01);

    // Airborne body dropped onto 60-deg slope slides down to flat ground (x <= 1.0, z == 0.0) without tunneling.
    let mut drop_state = BodyState::airborne(Vec3::new(2.0, 0.0, 2.5), Vec3::ZERO, 0.0);
    for _ in 0..180 {
        let res = step_kinematic_body(&cliff_world, shape, drop_state, DT, &params);
        drop_state = res.state;
        let ground_z = if drop_state.position.x <= 1.0 {
            0.0
        } else {
            (drop_state.position.x - 1.0) * cliff_slope
        };
        assert!(
            drop_state.position.z >= ground_z - 1e-3,
            "airborne slide tunneled below cliff: z={} < ground_z={}",
            drop_state.position.z,
            ground_z
        );
        if drop_state.motion.is_grounded() {
            break;
        }
    }
    assert!(
        drop_state.motion.is_grounded(),
        "must land once reaching flat bottom"
    );
    assert!(drop_state.position.x <= 1.05);
    assert!(drop_state.position.z.abs() < 1e-3);

    // 4. Ceiling head-bonk against overhead BuildingSlab.
    let mut ceil_world = SyntheticWorld::default();
    ceil_world.colliders.push(Collider {
        bounds: Aabb3::new(
            Vec3::new(-2.0, -2.0, 1.388 - 0.16),
            Vec3::new(2.0, 2.0, 1.388),
        ),
        kind: ColliderKind::BuildingSlab {
            building_id: 1,
            floor: 1,
            is_roof: false,
        },
        material: Material::WoodLight,
    });
    let jump_state = BodyState::airborne(Vec3::new(0.0, 0.0, 0.15), Vec3::new(0.0, 0.0, 6.0), 0.0);
    let mut bonked = false;
    let mut s = jump_state;
    for _ in 0..10 {
        let res = step_kinematic_body(&ceil_world, shape, s, DT, &params);
        s = res.state;
        if res.hit_ceiling {
            bonked = true;
            assert!(s.velocity.z <= 0.0);
            assert!(s.position.z + shape.height <= 1.388 - 0.16 + 1e-3);
            break;
        }
    }
    assert!(bonked, "upward motion must hit overhead BuildingSlab");
}

#[test]
fn high_speed_thin_wall_ccd_prevents_tunneling() {
    let shape = BodyShape::new(0.23, 1.05);
    let params = MoveParams::default();

    for (half_t, wall_along_y) in [(0.05_f32, true), (0.08_f32, true), (0.08_f32, false)] {
        let mut world = SyntheticWorld::default();
        let tile = Tile3D::new(5, 5, 0);
        let wall_bounds = if wall_along_y {
            Aabb3::new(
                Vec3::new(5.0 - half_t, 0.0, 0.0),
                Vec3::new(5.0 + half_t, 10.0, 1.5),
            )
        } else {
            Aabb3::new(
                Vec3::new(0.0, 5.0 - half_t, 0.0),
                Vec3::new(10.0, 5.0 + half_t, 1.5),
            )
        };
        world.colliders.push(Collider {
            bounds: wall_bounds,
            kind: ColliderKind::Wall { tile },
            material: Material::BrickMasonry,
        });

        for speed in [25.0_f32, 60.0, 250.0] {
            // Head-on approach (within 1-tick reach even at 25.0 u/s: 0.17 < 25*DT = 0.417).
            let (start, vel) = if wall_along_y {
                (Vec3::new(4.55, 5.0, 0.0), Vec2::new(speed, 0.0))
            } else {
                (Vec3::new(5.0, 4.55, 0.0), Vec2::new(0.0, speed))
            };
            let state = BodyState::grounded(start, 0.0, SupportId::Terrain);
            let res = move_kinematic_with_horizontal_vel(&world, shape, state, vel, DT, &params);
            assert!(res.blocked_xy);
            assert!(res.hit_colliders.contains(&ColliderKind::Wall { tile }));
            if wall_along_y {
                assert!(
                    res.state.position.x <= 5.0 - half_t - shape.radius + 1e-3,
                    "tunneled through AlongY thin wall at speed {speed}: x={}",
                    res.state.position.x
                );
            } else {
                assert!(
                    res.state.position.y <= 5.0 - half_t - shape.radius + 1e-3,
                    "tunneled through AlongX thin wall at speed {speed}: y={}",
                    res.state.position.y
                );
            }

            // 45-deg oblique approach slides cleanly parallel to wall without crossing.
            let comp = speed * std::f32::consts::FRAC_1_SQRT_2;
            let oblique_vel = Vec2::new(comp, comp);
            let oblique_res =
                move_kinematic_with_horizontal_vel(&world, shape, state, oblique_vel, DT, &params);
            assert!(oblique_res.blocked_xy);
            if wall_along_y {
                assert!(oblique_res.state.position.x <= 5.0 - half_t - shape.radius + 1e-3);
                assert!(oblique_res.state.position.y > start.y + 0.1);
            } else {
                assert!(oblique_res.state.position.y <= 5.0 - half_t - shape.radius + 1e-3);
                assert!(oblique_res.state.position.x > start.x + 0.1);
            }
        }
    }

    // Long 14.0-unit sphere_cast (rocket r=0.16, bolt r=0.14) never tunnels through 0.16 wall or 0.04 glass.
    let mut cast_world = SyntheticWorld::default();
    let wall_tile = Tile3D::new(10, 5, 0);
    cast_world.colliders.push(Collider {
        bounds: Aabb3::new(
            Vec3::new(10.0 - WALL_HALF_THICKNESS, 4.0, 0.0),
            Vec3::new(10.0 + WALL_HALF_THICKNESS, 6.0, 1.2),
        ),
        kind: ColliderKind::Wall { tile: wall_tile },
        material: Material::BrickMasonry,
    });
    for r in [0.16_f32, 0.14] {
        let hit = cast_world
            .sphere_cast(Vec3::new(3.0, 5.0, 0.65), Vec3::new(17.0, 5.0, 0.65), r)
            .expect("sphere_cast must hit thin wall");
        assert_eq!(
            hit.surface,
            HitSurface::Collider(ColliderKind::Wall { tile: wall_tile })
        );
        assert!((hit.point.x - (10.0 - WALL_HALF_THICKNESS)).abs() < 1e-4);
    }

    let mut glass_world = SyntheticWorld::default();
    let win_tile = Tile3D::new(10, 5, 0);
    glass_world.colliders.push(Collider {
        bounds: Aabb3::new(Vec3::new(9.98, 4.62, 0.38), Vec3::new(10.02, 5.38, 1.02)),
        kind: ColliderKind::WindowGlass { tile: win_tile },
        material: Material::Glass,
    });
    for r in [0.16_f32, 0.14] {
        let hit = glass_world
            .sphere_cast(Vec3::new(3.0, 5.0, 0.70), Vec3::new(17.0, 5.0, 0.70), r)
            .expect("sphere_cast must hit thin 0.04 WindowGlass");
        assert_eq!(
            hit.surface,
            HitSurface::Collider(ColliderKind::WindowGlass { tile: win_tile })
        );
    }
}

#[test]
fn birch_street_apertures_door_toggles_and_window_shatter() {
    let mut game = ActionGame::residential(42);
    let origin = game.block().unwrap().origin();

    // House 1 South window at local (7, 23, 0), AlongX.
    let win = Tile3D::new(origin.x + 7, origin.y + 23, 0);
    let wx = win.x as f32;
    let wy = win.y as f32;

    // 1. Window sill at z = 0.20.
    let sill_hit = game
        .spatial()
        .raycast(
            Ray3::new(Vec3::new(wx, wy + 2.0, 0.20), Vec3::new(0.0, -1.0, 0.0)),
            4.0,
        )
        .expect("hits sill");
    assert_eq!(
        sill_hit.surface,
        HitSurface::Collider(ColliderKind::WindowSill {
            tile: win,
            broken: false
        })
    );

    // 2. Window glass at z = 0.70 when intact, passes through on LINE_OF_SIGHT or after shatter.
    let glass_ray = Ray3::new(Vec3::new(wx, wy + 2.0, 0.70), Vec3::new(0.0, -1.0, 0.0));
    let glass_hit = game.spatial().raycast(glass_ray, 4.0).expect("hits glass");
    assert_eq!(
        glass_hit.surface,
        HitSurface::Collider(ColliderKind::WindowGlass { tile: win })
    );
    assert_eq!(glass_hit.material, Material::Glass);

    // LINE_OF_SIGHT ignores WindowGlass and crosses into room (hitting far wall/furniture > 2.2 units away).
    if let Some(los_hit) =
        game.spatial()
            .raycast_filtered(glass_ray, 4.0, QueryFilter::LINE_OF_SIGHT)
    {
        assert_ne!(
            los_hit.surface,
            HitSurface::Collider(ColliderKind::WindowGlass { tile: win })
        );
        assert!(los_hit.distance > 2.1);
    }

    // Shatter window live via damage_tile -> immediately passes through default raycast.
    let dmg = game
        .damage_tile(win, DamageKind::Bullet)
        .expect("damage window");
    assert!(dmg.destroyed);
    assert_eq!(dmg.new_kind, TileKind::BrokenWindow);
    if let Some(after_shatter) = game.spatial().raycast(glass_ray, 2.2) {
        panic!("expected broken window aperture to be open air, got {after_shatter:?}");
    }

    // 3. Upper lintel at z = 1.12.
    let lintel_hit = game
        .spatial()
        .raycast(
            Ray3::new(Vec3::new(wx, wy + 2.0, 1.12), Vec3::new(0.0, -1.0, 0.0)),
            4.0,
        )
        .expect("hits lintel");
    assert_eq!(
        lintel_hit.surface,
        HitSurface::Collider(ColliderKind::Wall { tile: win })
    );

    // 4. Ray through the upper floor slab.
    let slab_hit = game
        .spatial()
        .raycast(
            Ray3::new(
                Vec3::new(wx, wy + 2.0, FLOOR_H - 0.08),
                Vec3::new(0.0, -1.0, 0.0),
            ),
            4.0,
        )
        .expect("hits 2F slab");
    assert_eq!(
        slab_hit.surface,
        HitSurface::Collider(ColliderKind::BuildingSlab {
            building_id: 1,
            floor: 1,
            is_roof: false
        })
    );

    // 5. Door aperture toggle on House 1 back door local (11, 16, 0) (initially closed).
    let door = Tile3D::new(origin.x + 11, origin.y + 16, 0);
    let dx = door.x as f32;
    let dy = door.y as f32;
    let door_from = Vec3::new(dx, dy - 1.5, 0.65);
    let door_to = Vec3::new(dx, dy + 1.5, 0.65);

    let closed_hit = game
        .spatial()
        .sphere_cast(door_from, door_to, 0.16)
        .expect("closed door blocks sphere_cast");
    assert_eq!(
        closed_hit.surface,
        HitSurface::Collider(ColliderKind::ClosedDoor { tile: door })
    );

    assert_eq!(game.toggle_door(door), Some(true));
    assert!(
        game.spatial()
            .sphere_cast(door_from, door_to, 0.16)
            .is_none(),
        "open door aperture must let 0.16 sphere_cast pass straight through"
    );

    assert_eq!(game.toggle_door(door), Some(false));
    let reclosed = game
        .spatial()
        .sphere_cast(door_from, door_to, 0.16)
        .expect("re-closed door blocks again");
    assert_eq!(
        reclosed.surface,
        HitSurface::Collider(ColliderKind::ClosedDoor { tile: door })
    );
}

// Keep the live terrain quads while excluding props and building surfaces that
// can legitimately intercept a ray before it reaches the terrain.
struct TerrainOnly<'a, S>(&'a S);

impl<S: SpatialWorld> SpatialWorld for TerrainOnly<'_, S> {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
        self.0.terrain_at(xy)
    }

    fn supports_at(&self, _xy: Vec2, out: &mut Vec<SupportSurface>) {
        out.clear();
    }

    fn colliders_in(&self, _bounds: Aabb3, out: &mut Vec<Collider>) {
        out.clear();
    }

    fn surface_quads_in(&self, min: Position, max: Position, out: &mut Vec<SurfaceQuad>) {
        self.0.surface_quads_in(min, max, out);
        out.retain(|quad| matches!(quad.surface, HitSurface::Terrain { .. }));
    }
}

#[test]
fn county_seed_42_terrain_seam_and_diagonal_raycasts() {
    let game = ActionGame::county(42);
    let world = game.world();
    let live_spatial = game.spatial();
    let spatial = TerrainOnly(&live_spatial);

    let wake = world.wake_position();
    let base_x = (wake.x / 16) * 16;
    let base_y = (wake.y / 16) * 16;

    // Test vertical and oblique rays directly onto tile borders, chunk seams (x = k*16 - 0.5),
    // and canonical `00 -> 11` diagonals (`u == v`).
    let mut points = Vec::new();
    for dy in 0..8 {
        let ty = (base_y + dy).clamp(1, WORLD_SIZE - 2);
        let seam_x = (base_x + 16).clamp(1, WORLD_SIZE - 2) as f32 - 0.5;
        points.push(Vec2::new(seam_x, ty as f32));
        points.push(Vec2::new(seam_x, ty as f32 - 0.5));
        let tx = (base_x + dy).clamp(1, WORLD_SIZE - 2);
        points.push(Vec2::new(tx as f32 - 0.15, ty as f32 - 0.15)); // u == v = 0.35
        points.push(Vec2::new(tx as f32 + 0.25, ty as f32 + 0.25)); // u == v = 0.75
    }

    for pt in points {
        // Ensure outdoor tile (no building roof overhead) by skipping building tiles if any.
        let tile = Tile3D::from_vec2(pt, 0);
        if game
            .tile_render_info(tile.pos(), 0)
            .is_some_and(|ti| ti.building_id != 0)
        {
            continue;
        }
        let expected_z = world.elevation_at(pt).expect("valid terrain elevation");
        let vert_ray = Ray3::new(pt.extend(expected_z + 10.0), Vec3::new(0.0, 0.0, -1.0));
        let hit = spatial
            .raycast(vert_ray, 25.0)
            .expect("vertical ray hits terrain");
        assert!(
            (hit.point.z - expected_z).abs() < 1e-4,
            "vertical ray z mismatch at {pt:?}: hit={} expected={expected_z}",
            hit.point.z
        );

        let target = pt.extend(expected_z);
        let origin = Vec3::new(pt.x - 1.2, pt.y - 0.8, expected_z + 3.0);
        let (oblique_ray, _) = Ray3::from_points(origin, target).unwrap();
        let ohit = spatial
            .raycast(oblique_ray, 15.0)
            .expect("oblique ray hits terrain");
        let hit_expected_z = world.elevation_at(ohit.point.xy()).unwrap();
        assert!(
            (ohit.point.z - hit_expected_z).abs() < 1e-4,
            "oblique ray z mismatch: hit={ohit:?} target={target:?} expected={hit_expected_z}"
        );
    }
}

#[test]
fn birch_street_stairs_window_drop_and_collapse_fall() {
    let shape = BodyShape::new(0.23, 1.05);
    let params = MoveParams::default();

    // 1. Stair ascent and descent in House 1: (11, 21) -> (12, 21) -> (12, 20) on 2F, then back down.
    let mut game = ActionGame::residential(42);
    let origin = game.block().unwrap().origin();
    let spatial = game.spatial();

    let landing_f0 = Vec2::new((origin.x + 11) as f32, (origin.y + 21) as f32);
    let stair_xy = Vec2::new((origin.x + 12) as f32, (origin.y + 21) as f32);
    let exit_f1 = Vec2::new((origin.x + 12) as f32, (origin.y + 20) as f32);

    let mut state = BodyState::grounded(
        landing_f0.extend(0.0),
        0.0,
        SupportId::Floor(Tile3D::new(origin.x + 11, origin.y + 21, 0)),
    );
    let mut saw_stair = false;
    let mut max_z = 0.0_f32;

    // Walk East onto stair centerline.
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &spatial,
            shape,
            state,
            Vec2::new(2.5, 0.0),
            DT,
            &params,
        );
        state = res.state;
        assert!(state.motion.is_grounded());
        if matches!(state.motion, MotionState::Grounded(SupportId::Stair(_))) {
            saw_stair = true;
        }
        assert!(state.position.z >= max_z - 1e-4);
        max_z = max_z.max(state.position.z);
        if (state.position.x - stair_xy.x).abs() < 0.05 {
            break;
        }
    }
    assert!(saw_stair, "must engage SupportId::Stair");

    // Turn North onto 2F landing (12, 20).
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &spatial,
            shape,
            state,
            Vec2::new(0.0, -2.5),
            DT,
            &params,
        );
        state = res.state;
        assert!(state.motion.is_grounded());
        max_z = max_z.max(state.position.z);
        if state.position.y <= exit_f1.y + 0.1 {
            break;
        }
    }
    assert!((state.position.z - FLOOR_H).abs() < 1e-3);
    assert_eq!(
        state.motion,
        MotionState::Grounded(SupportId::Floor(Tile3D::new(
            origin.x + 12,
            origin.y + 20,
            1
        )))
    );

    // Walk back down to F0 landing.
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &spatial,
            shape,
            state,
            Vec2::new(0.0, 2.5),
            DT,
            &params,
        );
        state = res.state;
        assert!(state.motion.is_grounded());
        if (state.position.y - stair_xy.y).abs() < 0.05 {
            break;
        }
    }
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &spatial,
            shape,
            state,
            Vec2::new(-2.5, 0.0),
            DT,
            &params,
        );
        state = res.state;
        assert!(state.motion.is_grounded());
        if state.position.x <= landing_f0.x + 0.1 {
            break;
        }
    }
    assert!(state.position.z.abs() < 1e-3);
    assert_eq!(
        state.motion,
        MotionState::Grounded(SupportId::Floor(Tile3D::new(
            origin.x + 11,
            origin.y + 21,
            0
        )))
    );

    // 2. 2F Broken Window drop & gravity landing.
    let f1_win = Tile3D::new(origin.x + 7, origin.y + 23, 1);
    let dmg = game.damage_tile(f1_win, DamageKind::Bullet).unwrap();
    assert!(dmg.destroyed);

    let spatial = game.spatial();
    // Place airborne/dropping just outside the 2F broken window aperture at z = FLOOR_H + 0.40 -> lands on yard z = 0.0.
    let mut drop_state = BodyState::airborne(
        Vec3::new(f1_win.x as f32, f1_win.y as f32 + 0.30, FLOOR_H + 0.40),
        Vec3::new(0.0, 2.5, 0.0),
        0.0,
    );
    let mut impact = 0.0_f32;
    for _ in 0..120 {
        let res = step_kinematic_body(&spatial, shape, drop_state, DT, &params);
        drop_state = res.state;
        impact = impact.max(res.landed_impact_speed);
        if drop_state.motion.is_grounded() {
            break;
        }
    }
    assert!(drop_state.motion.is_grounded());
    assert!(drop_state.position.z.abs() < 1e-4);
    assert!(impact > 5.0, "expected fall impact > 5.0 u/s, got {impact}");

    // 3. Load-bearing wall breach & immediate collapse fall.
    let mut collapse_game = ActionGame::residential(42);
    // Stand stationary on House 1 F1 empty tile (7, 17, 1) next to NW corner, and breach F0 walls.
    let f1_stand = Tile3D::new(origin.x + 7, origin.y + 17, 1);
    // First breach NW corner supports (5,16), (6,16), (5,17), (8,16) so F1 cells collapse.
    let f0_walls = [
        Tile3D::new(origin.x + 5, origin.y + 16, 0),
        Tile3D::new(origin.x + 6, origin.y + 16, 0),
        Tile3D::new(origin.x + 5, origin.y + 17, 0),
        Tile3D::new(origin.x + 8, origin.y + 16, 0),
        Tile3D::new(origin.x + 9, origin.y + 16, 0),
    ];

    // Find an interior F1 tile that is currently Floor(1) and collapses when those F0 walls are breached.
    let mut pre_supports = Vec::new();
    collapse_game.spatial().supports_at(
        Vec2::new(f1_stand.x as f32, f1_stand.y as f32),
        &mut pre_supports,
    );
    assert!(
        pre_supports
            .iter()
            .any(|s| s.id == SupportId::Floor(f1_stand)),
        "F1 tile must have floor support before breach"
    );

    let mut stand_state = BodyState::grounded(
        Vec3::new(f1_stand.x as f32, f1_stand.y as f32, FLOOR_H),
        0.0,
        SupportId::Floor(f1_stand),
    );

    let mut total_collapsed = 0u8;
    for w in f0_walls {
        if let Some(r) = collapse_game.damage_tile(w, DamageKind::RocketBlast) {
            total_collapsed = total_collapsed.saturating_add(r.collapsed_cells);
        }
    }
    assert!(
        total_collapsed > 0,
        "breaching F0 load-bearing walls must trigger F1 collapse"
    );

    // Find a collapsed F1 tile where floor support is now gone (or check f1_stand).
    let mut post_supports = Vec::new();
    collapse_game.spatial().supports_at(
        Vec2::new(f1_stand.x as f32, f1_stand.y as f32),
        &mut post_supports,
    );
    let collapsed_tile = if !post_supports
        .iter()
        .any(|s| s.id == SupportId::Floor(f1_stand))
    {
        f1_stand
    } else {
        // Pick one of the collapsed tiles directly from the block.
        let mut found = None;
        for dy in 16..=23 {
            for dx in 5..=13 {
                let t = Tile3D::new(origin.x + dx, origin.y + dy, 1);
                if let Some(ti) = collapse_game.tile_render_info(t.pos(), 1)
                    && ti.surface == Surface::Air
                {
                    found = Some(t);
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let ct = found.expect("at least one collapsed F1 tile");
        stand_state = BodyState::grounded(
            Vec3::new(ct.x as f32, ct.y as f32, FLOOR_H),
            0.0,
            SupportId::Floor(ct),
        );
        ct
    };
    let _ = collapsed_tile;

    let live_spatial = collapse_game.spatial();
    let tick1 = step_kinematic_body(&live_spatial, shape, stand_state, DT, &params);
    assert_eq!(
        tick1.state.motion,
        MotionState::Airborne,
        "stationary body on collapsed F1 cell must become Airborne on tick 1"
    );

    let mut fall_state = tick1.state;
    let mut fall_impact = tick1.landed_impact_speed;
    for _ in 0..120 {
        let res = step_kinematic_body(&live_spatial, shape, fall_state, DT, &params);
        fall_state = res.state;
        fall_impact = fall_impact.max(res.landed_impact_speed);
        if fall_state.motion.is_grounded() {
            break;
        }
    }
    assert!(fall_state.motion.is_grounded());
    assert!(fall_state.position.z.abs() < 1e-4);
    assert!(
        fall_impact > 5.0,
        "collapse fall must record positive impact speed"
    );
}

#[test]
fn falling_under_stair_ramp_lands_on_the_floor_below() {
    let game = ActionGame::residential(42);
    let origin = game.block().unwrap().origin();
    let stair = Tile3D::new(origin.x + 12, origin.y + 21, 0);
    let shape = BodyShape::new(0.23, 1.05);
    let spatial = game.spatial();
    let stair_center = stair.center();
    let mut supports = Vec::new();
    spatial.supports_at(stair_center, &mut supports);
    assert!(supports.iter().any(|s| s.is_stair));

    // A body that has slipped beneath the winder is below the ramp surface. The
    // floor plane under the ramp must still catch it instead of letting it fall
    // through the entire house to the terrain.
    let mut state = BodyState::airborne(
        stair_center.extend(FLOOR_H * 0.3),
        Vec3::new(0.0, 0.0, -0.5),
        0.0,
    );
    let params = MoveParams::default();
    for _ in 0..30 {
        state = step_kinematic_body(&spatial, shape, state, DT, &params).state;
        if state.motion.is_grounded() {
            break;
        }
    }
    assert!(
        state.motion.is_grounded(),
        "body must land on the stairwell floor"
    );
    assert_eq!(state.motion, MotionState::Grounded(SupportId::Floor(stair)));
    assert!(state.position.z.abs() < 1e-4);
    let landed_x = state.position.x;
    for _ in 0..20 {
        let res = move_kinematic_with_horizontal_vel(
            &spatial,
            shape,
            state,
            Vec2::new(-1.5, 0.0),
            DT,
            &params,
        );
        assert_eq!(res.landed_impact_speed, 0.0);
        state = res.state;
        assert!(state.motion.is_grounded());
    }
    assert!(
        state.position.x < landed_x - 0.2,
        "player must walk out from under the ramp"
    );
}

#[test]
fn county_seed_42_bridge_underpass_and_3story_apartments() {
    let mut game = ActionGame::county(42);
    let world = game.world().clone();
    let shape = BodyShape::new(0.23, 1.05);
    let params = MoveParams::default();

    // 1. Locate a road->bridge approach tile and a mid-span bridge tile with open water edge in Seed 42.
    let mut approach_bridge = None;
    let mut approach_dir = None;
    let mut parapet_bridge = None;
    let mut water_side = None;

    for y in 8..WORLD_SIZE - 8 {
        for x in 8..WORLD_SIZE - 8 {
            let p = Position::new(x, y);
            if !world.tile_at(p).is_some_and(|t| t.ground == Ground::Bridge) {
                continue;
            }
            if approach_bridge.is_none() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let q = Position::new(x + dx, y + dy);
                    if world
                        .tile_at(q)
                        .is_some_and(|t| t.ground.is_road() && t.ground != Ground::Bridge)
                    {
                        approach_bridge = Some(p);
                        approach_dir = Some((-dx, -dy));
                        break;
                    }
                }
            }
            if parapet_bridge.is_none()
                && world
                    .elevation_at(Vec2::new(x as f32, y as f32))
                    .is_some_and(|z| z <= -0.5)
            {
                for (sdx, sdy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                    let sq = Position::new(x + sdx, y + sdy);
                    if world.tile_at(sq).is_some_and(|t| !t.ground.is_road()) {
                        parapet_bridge = Some(p);
                        water_side = Some((sdx, sdy));
                        break;
                    }
                }
            }
            if approach_bridge.is_some() && parapet_bridge.is_some() {
                break;
            }
        }
        if approach_bridge.is_some() && parapet_bridge.is_some() {
            break;
        }
    }
    let bp_approach = approach_bridge.expect("Seed 42 has a bridge approach");
    let (bdx, bdy) = approach_dir.unwrap();
    let bp = parapet_bridge.expect("Seed 42 has a mid-span bridge tile over water");
    let (px_side, py_side) = water_side.unwrap();
    let approach = Position::new(bp_approach.x - bdx, bp_approach.y - bdy);
    let spatial = game.spatial();

    // Walk road -> bridge -> across bridge.
    let start_z = world
        .elevation_at(Vec2::new(approach.x as f32, approach.y as f32))
        .unwrap();
    let mut deck_state = BodyState::grounded(
        Vec3::new(approach.x as f32, approach.y as f32, start_z),
        0.0,
        SupportId::Terrain,
    );
    let mut saw_bridge = false;
    let vel = Vec2::new(bdx as f32 * 4.0, bdy as f32 * 4.0);
    for _ in 0..60 {
        let res = move_kinematic_with_horizontal_vel(&spatial, shape, deck_state, vel, DT, &params);
        deck_state = res.state;
        assert!(deck_state.motion.is_grounded());
        if matches!(
            deck_state.motion,
            MotionState::Grounded(SupportId::Bridge(_))
        ) {
            saw_bridge = true;
            assert!(
                deck_state.position.z >= 0.90,
                "bridge deck elevation must be >= 0.90, got {}",
                deck_state.position.z
            );
        }
    }
    assert!(
        saw_bridge,
        "body walking across road onto bridge must traverse SupportId::Bridge"
    );

    // Check bridge parapet blocking on open water side of `bp`.
    let bridge_deck_z = world
        .bridge_deck_at(Vec2::new(bp.x as f32, bp.y as f32))
        .unwrap()
        .z;
    let parapet_start = BodyState::grounded(
        Vec3::new(bp.x as f32, bp.y as f32, bridge_deck_z),
        0.0,
        SupportId::Bridge(bp),
    );
    let mut parapet_blocked = false;
    let mut ps = parapet_start;
    for _ in 0..20 {
        let res = move_kinematic_with_horizontal_vel(
            &spatial,
            shape,
            ps,
            Vec2::new(px_side as f32 * 3.0, py_side as f32 * 3.0),
            DT,
            &params,
        );
        ps = res.state;
        if res.blocked_xy
            && res
                .hit_colliders
                .contains(&ColliderKind::BridgeParapet { tile: bp })
        {
            parapet_blocked = true;
            break;
        }
    }
    assert!(
        parapet_blocked,
        "bridge open water edge must have a blocking BridgeParapet"
    );

    // Underpass anti-snap: walk along riverbed underneath `bp` with avoid_water = false.
    let bed_z = world
        .elevation_at(Vec2::new(bp.x as f32, bp.y as f32))
        .unwrap();
    assert!(
        bed_z <= -0.5,
        "riverbed under bridge must be submerged, got {bed_z}"
    );

    let under_params = MoveParams {
        avoid_water: false,
        ..params
    };
    let mut under_state = BodyState::grounded(
        Vec3::new(bp.x as f32, bp.y as f32, bed_z),
        0.0,
        SupportId::Terrain,
    );
    for _ in 0..25 {
        let res = move_kinematic_with_horizontal_vel(
            &spatial,
            shape,
            under_state,
            Vec2::new(px_side as f32 * 2.0, py_side as f32 * 2.0),
            DT,
            &under_params,
        );
        under_state = res.state;
        assert_eq!(
            under_state.motion,
            MotionState::Grounded(SupportId::Terrain),
            "underpass body must stay on SupportId::Terrain without snapping to Bridge"
        );
        assert!(under_state.position.z < 0.2);
    }

    // Upward ray from riverbed under bridge hits ColliderKind::BridgeDeck.
    let up_hit = spatial
        .raycast(
            Ray3::new(Vec3::new(bp.x as f32, bp.y as f32, bed_z + 0.1), Vec3::Z),
            5.0,
        )
        .expect("upward ray under bridge hits BridgeDeck");
    assert_eq!(
        up_hit.surface,
        HitSurface::Collider(ColliderKind::BridgeDeck { tile: bp })
    );

    // 2. 3-Story Downtown Apartments Stairs in Seed 42 (F0 -> F1 -> F2 -> F0).
    let apt = world
        .sites()
        .unwrap()
        .buildings
        .iter()
        .find(|b| b.floors >= 3)
        .expect("Seed 42 has a 3-story Apartments building");
    let stair_pos = Position::new(apt.min.x + 1, apt.min.y + 1);
    game.teleport(Tile3D::from_pos(apt.entry, 0))
        .expect("teleport near 3-story apartments");

    let apt_spatial = game.spatial();
    let fz = apt.foundation_z;

    // Start on F0 East of stair (min.x + 2, min.y + 1), walk West onto stair, then South onto F1 (min.x + 1, min.y + 2).
    let mut apt_state = BodyState::grounded(
        Vec3::new((stair_pos.x + 1) as f32, stair_pos.y as f32, fz),
        0.0,
        SupportId::Floor(Tile3D::new(stair_pos.x + 1, stair_pos.y, 0)),
    );

    // Walk West into (min.x + 1, min.y + 1) on F0 -> F1.
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(-2.5, 0.0),
            DT,
            &params,
        );
        apt_state = res.state;
        assert!(apt_state.motion.is_grounded());
        if (apt_state.position.x - stair_pos.x as f32).abs() < 0.05 {
            break;
        }
    }
    assert_eq!(
        apt_state.motion,
        MotionState::Grounded(SupportId::Stair(Tile3D::from_pos(stair_pos, 0)))
    );

    // Turn South onto F1 (min.x + 1, min.y + 2).
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(0.0, 2.5),
            DT,
            &params,
        );
        apt_state = res.state;
        assert!(apt_state.motion.is_grounded());
        if apt_state.position.y >= (stair_pos.y + 1) as f32 - 0.05 {
            break;
        }
    }
    assert!(
        (apt_state.position.z - floor_elevation(fz, 1)).abs() < 1e-3,
        "expected F1 elevation {}, got {}",
        floor_elevation(fz, 1),
        apt_state.position.z
    );

    // Walk onto F1 landing (min.x + 2, min.y + 1) then West onto StairsBoth (1 -> 2) and South onto F2!
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(2.5, 0.0),
            DT,
            &params,
        );
        apt_state = res.state;
        if apt_state.position.x >= (stair_pos.x + 1) as f32 - 0.05 {
            break;
        }
    }
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(0.0, -2.5),
            DT,
            &params,
        );
        apt_state = res.state;
        if (apt_state.position.y - stair_pos.y as f32).abs() < 0.05 {
            break;
        }
    }
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(-2.5, 0.0),
            DT,
            &params,
        );
        apt_state = res.state;
        assert!(apt_state.motion.is_grounded());
        if (apt_state.position.x - stair_pos.x as f32).abs() < 0.05 {
            break;
        }
    }
    assert_eq!(
        apt_state.motion,
        MotionState::Grounded(SupportId::Stair(Tile3D::from_pos(stair_pos, 1)))
    );
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(0.0, 2.5),
            DT,
            &params,
        );
        apt_state = res.state;
        assert!(apt_state.motion.is_grounded());
        if apt_state.position.y >= (stair_pos.y + 1) as f32 - 0.05 {
            break;
        }
    }
    assert!(
        (apt_state.position.z - floor_elevation(fz, 2)).abs() < 1e-3,
        "expected F2 elevation {}, got {}",
        floor_elevation(fz, 2),
        apt_state.position.z
    );
    assert_eq!(
        apt_state.motion,
        MotionState::Grounded(SupportId::Floor(Tile3D::new(
            stair_pos.x,
            stair_pos.y + 1,
            2
        )))
    );

    // Descend F2 -> F1 -> F0 via StairsBoth and StairsUp.
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(0.0, -2.5),
            DT,
            &params,
        );
        apt_state = res.state;
        assert!(apt_state.motion.is_grounded());
        if (apt_state.position.y - stair_pos.y as f32).abs() < 0.05 {
            break;
        }
    }
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(2.5, 0.0),
            DT,
            &params,
        );
        apt_state = res.state;
        assert!(apt_state.motion.is_grounded());
        if apt_state.position.x >= (stair_pos.x + 1) as f32 - 0.05 {
            break;
        }
    }
    assert!((apt_state.position.z - floor_elevation(fz, 1)).abs() < 1e-3);

    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(0.0, 2.5),
            DT,
            &params,
        );
        apt_state = res.state;
        if apt_state.position.y >= (stair_pos.y + 1) as f32 - 0.05 {
            break;
        }
    }
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(-2.5, 0.0),
            DT,
            &params,
        );
        apt_state = res.state;
        if (apt_state.position.x - stair_pos.x as f32).abs() < 0.05 {
            break;
        }
    }
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(0.0, -2.5),
            DT,
            &params,
        );
        apt_state = res.state;
        assert!(apt_state.motion.is_grounded());
        if (apt_state.position.y - stair_pos.y as f32).abs() < 0.05 {
            break;
        }
    }
    for _ in 0..45 {
        let res = move_kinematic_with_horizontal_vel(
            &apt_spatial,
            shape,
            apt_state,
            Vec2::new(2.5, 0.0),
            DT,
            &params,
        );
        apt_state = res.state;
        assert!(apt_state.motion.is_grounded());
        if apt_state.position.x >= (stair_pos.x + 1) as f32 - 0.05 {
            break;
        }
    }
    assert!(
        (apt_state.position.z - fz).abs() < 1e-3,
        "expected F0 elevation {fz}, got {}",
        apt_state.position.z
    );
    assert_eq!(
        apt_state.motion,
        MotionState::Grounded(SupportId::Floor(Tile3D::new(
            stair_pos.x + 1,
            stair_pos.y,
            0
        )))
    );
}
