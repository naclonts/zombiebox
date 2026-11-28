use std::collections::{HashSet, VecDeque};

use zombiebox_core::{
    ActionGame, ActionInput, ControlMode, InteractPrompt, LandmarkKind, MotorcycleInput, Position,
    RideInput, Tile3D, Vec2, VehicleId, VehicleView, World, world_to_tile_coord,
};

const REVIEW_SEEDS: [u64; 6] = [0, 1, 42, 641, 93513, u64::MAX];

fn is_public_road(world: &World, p: Position) -> bool {
    world.tile_at(p).is_some_and(|tile| tile.ground.is_road())
}

fn road_component(world: &World, start: Position) -> HashSet<Position> {
    assert!(
        is_public_road(world, start),
        "BFS start {start:?} is not road"
    );
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(p) = queue.pop_front() {
        for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
            let q = Position::new(p.x + dx, p.y + dy);
            if is_public_road(world, q) && seen.insert(q) {
                queue.push_back(q);
            }
        }
    }
    seen
}

fn center_height(world: &World, p: Position) -> f32 {
    world
        .elevation_at(Vec2::new(p.x as f32, p.y as f32))
        .expect("in-bounds road sample")
}

#[test]
fn default_world_has_a_repeatable_highway_ribbon_plan() {
    for seed in (0..64).chain([u64::MAX]) {
        let world = World::new(seed);
        let replay = World::new(seed);
        let plan = world.town_plan().expect("default world has ribbon plan");
        let replay_plan = replay.town_plan().expect("replayed default ribbon plan");
        assert_eq!(world.landmarks(), replay.landmarks(), "seed {seed}");
        assert_eq!(
            plan.canonical_hash(),
            replay_plan.canonical_hash(),
            "seed {seed}"
        );
        assert_eq!(plan.roads.len(), 3, "seed {seed}");
        assert_eq!(plan.roads[0].class as u8, 0, "seed {seed}: arterial");
        assert_eq!(plan.roads[1].class as u8, 1, "seed {seed}: collector");
        assert_eq!(plan.roads[2].class as u8, 2, "seed {seed}: local");
        assert!(plan.frontages.len() >= 4, "seed {seed}: frontage slots");
    }
}

#[test]
fn public_road_raster_is_dry_connected_and_reaches_town_anchors() {
    for seed in REVIEW_SEEDS {
        let world = World::new(seed);
        let plan = world.town_plan().unwrap();
        let spawn = world.wake_position();
        // The residential entrance is the paved 5x5 turnaround at the local tip.
        assert_eq!(spawn, plan.local_junction, "seed {seed}");
        assert!(
            is_public_road(&world, spawn),
            "seed {seed}: spawn road tile"
        );
        let seen = road_component(&world, spawn);
        let arterial_mid = plan.roads[0].centerline[plan.roads[0].centerline.len() / 2];
        for target in [
            plan.collector_junction,
            plan.local_junction,
            plan.turnaround,
            arterial_mid,
        ] {
            assert!(
                seen.contains(&target),
                "seed {seed}: road route misses {target:?}"
            );
        }
        for landmark in world.landmarks() {
            if matches!(
                landmark.kind,
                LandmarkKind::Residential
                    | LandmarkKind::Downtown
                    | LandmarkKind::Hospital
                    | LandmarkKind::Police
            ) {
                assert!(
                    seen.contains(&landmark.entrance),
                    "seed {seed}: {:?} is off the connected road raster",
                    landmark.kind
                );
            }
        }
        for (p, cell) in plan.raster() {
            let tile = world.tile_at(p).expect("road raster within county");
            assert!(
                tile.ground.is_road(),
                "seed {seed}: planned road {p:?} missing"
            );
            assert!(
                tile.water.is_none(),
                "seed {seed}: wet road raster at {p:?}"
            );
            let class = plan.roads[cell.road_index].class as u8;
            if class == 0 {
                assert!(tile.ground.is_road(), "seed {seed}: arterial material");
            }
        }
    }
}

#[test]
fn road_grades_meet_class_caps_at_full_and_half_steps() {
    for seed in REVIEW_SEEDS {
        let world = World::new(seed);
        let plan = world.town_plan().unwrap();
        for road in &plan.roads {
            let limit = match road.class as u8 {
                0 => 0.09,
                _ => 0.12,
            };
            for (segment, pair) in road.centerline.windows(2).enumerate() {
                let a = pair[0];
                let b = pair[1];
                let dx = (b.x - a.x) as f32;
                let dy = (b.y - a.y) as f32;
                let length = dx.hypot(dy);
                let za = center_height(&world, a);
                let zb = center_height(&world, b);
                assert!(
                    (zb - za).abs() / length <= limit + 0.002,
                    "seed {seed} {:?} segment {segment}: full-step grade {:.4} > {limit}",
                    road.class,
                    (zb - za).abs() / length
                );
                let midpoint = Vec2::new(a.x as f32 + dx * 0.5, a.y as f32 + dy * 0.5);
                let zm = world.elevation_at(midpoint).unwrap();
                let half_length = length * 0.5;
                for grade in [(zm - za).abs() / half_length, (zb - zm).abs() / half_length] {
                    assert!(
                        grade <= limit + 0.003,
                        "seed {seed} {:?} segment {segment}: half-step grade {grade:.4} > {limit}",
                        road.class
                    );
                }
            }
        }
    }
}

#[test]
fn frontage_slots_are_dry_disjoint_and_connected_by_short_aprons() {
    for seed in REVIEW_SEEDS {
        let world = World::new(seed);
        let plan = world.town_plan().unwrap();
        for (i, slot) in plan.frontages.iter().enumerate() {
            assert!(
                slot.suitability.is_finite() && slot.suitability > 0.0,
                "seed {seed}"
            );
            assert!(slot.foundation_z.is_finite(), "seed {seed}");
            assert!(
                slot.bounds.width() >= 12 && slot.bounds.height() >= 12,
                "seed {seed}"
            );
            for y in slot.bounds.min.y..=slot.bounds.max.y {
                for x in slot.bounds.min.x..=slot.bounds.max.x {
                    let p = Position::new(x, y);
                    let tile = world.tile_at(p).unwrap();
                    assert!(tile.water.is_none(), "seed {seed}: wet slot tile {p:?}");
                    assert!(
                        !tile.ground.is_road(),
                        "seed {seed}: slot overlaps road at {p:?}"
                    );
                }
            }
            assert!(plan.road_at(slot.connector.road).is_some(), "seed {seed}");
            let distance = (slot.connector.road.x - slot.connector.frontage.x).abs()
                + (slot.connector.road.y - slot.connector.frontage.y).abs();
            assert!(
                (1..=4).contains(&distance),
                "seed {seed}: connector length {distance}"
            );
            assert!(slot.bounds.contains(slot.connector.frontage), "seed {seed}");
            let road_z = center_height(&world, slot.connector.road);
            assert!(
                (slot.foundation_z - road_z).abs() <= 0.12 * distance as f32 + 0.01,
                "seed {seed}: frontage/road grade exceeds driveway tolerance"
            );
            for other in plan.frontages.iter().skip(i + 1) {
                assert!(
                    !slot.bounds.intersects(other.bounds),
                    "seed {seed}: slot overlap"
                );
                let steps = distance;
                let dx = (slot.connector.frontage.x - slot.connector.road.x).signum();
                let dy = (slot.connector.frontage.y - slot.connector.road.y).signum();
                for step in 1..steps {
                    let apron = Position::new(
                        slot.connector.road.x + dx * step,
                        slot.connector.road.y + dy * step,
                    );
                    assert!(
                        !other.bounds.contains(apron),
                        "seed {seed}: connector for frontage {} crosses slot {}",
                        slot.id,
                        other.id
                    );
                }
            }
        }
    }
}

#[test]
fn arterial_is_sweeping_and_keeps_a_motorcycle_width() {
    for seed in REVIEW_SEEDS {
        let world = World::new(seed);
        let plan = world.town_plan().unwrap();
        let arterial = &plan.roads[0];
        let mut maximum_curvature = 0.0f32;
        for triple in arterial.continuous_centerline.windows(3) {
            let [a, b, c] = [triple[0], triple[1], triple[2]];
            let ab = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let bc = ((c[0] - b[0]).powi(2) + (c[1] - b[1]).powi(2)).sqrt();
            let ca = ((a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2)).sqrt();
            let twice_area = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
            if twice_area > 1.0e-5 {
                let radius = ab * bc * ca / (2.0 * twice_area);
                maximum_curvature = maximum_curvature.max(1.0 / radius);
            }
        }
        assert!(
            maximum_curvature <= 1.0 / 18.0 + 0.002,
            "seed {seed}: R_min {:.2}",
            1.0 / maximum_curvature.max(1e-6)
        );

        for pair in arterial.centerline.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let dx = (b.x - a.x) as f32;
            let dy = (b.y - a.y) as f32;
            let length = dx.hypot(dy).max(1e-4);
            let nx = -dy / length;
            let ny = dx / length;
            for side in [-2.0, -1.0, 0.0, 1.0, 2.0] {
                let p = Position::new(
                    (a.x as f32 + dx * 0.5 + nx * side).round() as i32,
                    (a.y as f32 + dy * 0.5 + ny * side).round() as i32,
                );
                assert!(
                    plan.road_at(p)
                        .is_some_and(|cell| { plan.roads[cell.road_index].class as u8 == 0 }),
                    "seed {seed}: arterial raster width pinches near {p:?}"
                );
                assert!(
                    world.tile_at(p).unwrap().ground.is_road(),
                    "seed {seed}: arterial width pinches near {p:?}"
                );
            }
        }
    }
}

fn starter_bike(game: &ActionGame) -> (VehicleId, Vec2) {
    game.vehicle_views()
        .find_map(|(id, view)| match view {
            VehicleView::Motorcycle(bike) if id.0 & 0xff == 0 => {
                Some((id, bike.body.position.xy()))
            }
            _ => None,
        })
        .expect("active run has its nearby starter motorcycle")
}

fn wrap_angle(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI
}

fn mounted_heading(game: &ActionGame, id: VehicleId) -> f32 {
    game.vehicle_views()
        .find_map(|(vehicle_id, view)| match view {
            VehicleView::Motorcycle(bike) if vehicle_id == id => Some(bike.body.heading),
            _ => None,
        })
        .expect("mounted motorcycle remains in the active view")
}

fn reverse_to_waypoint(game: &mut ActionGame, id: VehicleId, target: Position) -> f32 {
    let target = Vec2::new(target.x as f32, target.y as f32);
    let mut closest = f32::INFINITY;
    for _ in 0..9000 {
        let pose = game.player_pose();
        let delta = target - pose.position;
        let distance = delta.length();
        closest = closest.min(distance);
        let Some(mounted) = game.hud().mounted else {
            eprintln!("lost bike before waypoint {target:?} at {:?}", game.player_pose().position);
            break;
        };
        let speed = mounted.speed_mph_x10;
        if distance <= 3.0 && speed <= 30 {
            break;
        }
        let desired_heading = delta.y.atan2(delta.x) + std::f32::consts::PI;
        let heading_error = wrap_angle(desired_heading - mounted_heading(game, id));
        let steer = (-heading_error * 1.4).clamp(-0.8, 0.8);
        let input = if distance <= 3.0 {
            MotorcycleInput::default()
        } else {
            MotorcycleInput {
                brake: 1.0,
                steer,
                ..MotorcycleInput::default()
            }
        };
        game.step(ActionInput {
            ride: Some(RideInput::Motorcycle(input)),
            ..ActionInput::default()
        });
    }
    game.player_pose().position.distance(target).min(closest)
}

#[test]
fn mounted_bike_can_enter_and_follow_the_seed_42_ribbon() {
    let mut game = ActionGame::county(42);
    let (id, bike) = starter_bike(&game);
    let world = World::new(42);
    assert!(game.teleport(Tile3D::from_vec2(bike, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    assert_eq!(game.mounted_vehicle(), Some(id));
    let start = game.player_pose().position;
    let before = game.hud().mounted.expect("mounted HUD");
    let mut paved_steps = 0;
    for _ in 0..100 {
        game.step(ActionInput {
            ride: Some(RideInput::Motorcycle(MotorcycleInput {
                throttle: 0.65,
                ..MotorcycleInput::default()
            })),
            ..ActionInput::default()
        });
        let p = game.player_pose().position;
        let tile = Position::new(world_to_tile_coord(p.x), world_to_tile_coord(p.y));
        if world.tile_at(tile).is_some_and(|t| t.ground.is_road()) {
            paved_steps += 1;
        }
    }
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    assert!(
        game.player_pose().position.distance(start) > 3.0,
        "mounted bike should travel"
    );
    assert!(game.hud().mounted.unwrap().speed_mph_x10 > before.speed_mph_x10);
    assert!(
        paved_steps >= 80,
        "bike should follow the nearby paved ribbon: {paved_steps}/100 road samples"
    );
    assert!(matches!(
        game.hud().interact_prompt,
        InteractPrompt::Dismount | InteractPrompt::None
    ));
}

#[test]
fn mounted_bike_reaches_the_local_turnaround_on_seed_42() {
    let world = World::new(42);
    let plan = world.town_plan().unwrap();
    let start = plan.local_junction;
    let end = plan.turnaround;
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    let length = dx.hypot(dy);
    let dir = Vec2::new(dx / length, dy / length);

    let mut game = ActionGame::county(42);
    let (id, bike) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(bike, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.mounted_vehicle(), Some(id));

    let mut max_progress = 0.0f32;
    let mut paved_samples = 0;
    for _ in 0..3000 {
        let pos = game.player_pose().position;
        let progress = (pos.x - start.x as f32) * dir.x + (pos.y - start.y as f32) * dir.y;
        let remaining = length - progress;
        max_progress = max_progress.max(progress);
        let speed = game.hud().mounted.unwrap().speed_mph_x10;
        if remaining <= 1.0 && speed <= 10 {
            break;
        }
        let input = if remaining <= 1.0 {
            MotorcycleInput {
                brake: if speed > 10 { 1.0 } else { 0.0 },
                ..MotorcycleInput::default()
            }
        } else if speed < 45 {
            MotorcycleInput {
                throttle: 0.65,
                ..MotorcycleInput::default()
            }
        } else if speed > 50 {
            MotorcycleInput {
                brake: 0.25,
                ..MotorcycleInput::default()
            }
        } else {
            MotorcycleInput::default()
        };
        game.step(ActionInput {
            ride: Some(RideInput::Motorcycle(input)),
            ..ActionInput::default()
        });
        let pos = game.player_pose().position;
        let tile = Position::new(world_to_tile_coord(pos.x), world_to_tile_coord(pos.y));
        if world
            .tile_at(tile)
            .is_some_and(|tile| tile.ground.is_road())
        {
            paved_samples += 1;
        }
    }
    let final_pos = game.player_pose().position;
    let heading_before_turn = game.player_body().heading;
    let final_remaining =
        length - ((final_pos.x - start.x as f32) * dir.x + (final_pos.y - start.y as f32) * dir.y);
    eprintln!(
        "turnaround drive: {start:?}->{end:?}, pos {final_pos:?}, progress {max_progress:.1}/{length:.1}, remaining {final_remaining:.1}, speed {} x10 mph, paved {paved_samples}",
        game.hud().mounted.unwrap().speed_mph_x10
    );
    assert!(
        max_progress >= length - 4.0,
        "bike should reach the turnaround bulb"
    );
    assert!(
        final_remaining.abs() <= 5.0,
        "bike should stop at the turnaround"
    );
    assert!(
        paved_samples >= 150,
        "bike should remain on the paved local street"
    );

    let local_return = reverse_to_waypoint(&mut game, id, plan.local_junction);
    assert!(local_return <= 5.0, "bike should return to local junction: {local_return:.1}");
    let collector_return = reverse_to_waypoint(&mut game, id, plan.collector_junction);
    assert!(collector_return <= 5.0, "bike should follow collector to arterial: {collector_return:.1}");

    let arterial = &plan.roads[0].centerline;
    let junction_index = arterial
        .iter()
        .position(|&point| point == plan.collector_junction)
        .expect("collector joins the arterial centerline");
    let arterial_target = if junction_index + 20 < arterial.len() {
        arterial[junction_index + 20]
    } else {
        arterial[junction_index - 20]
    };
    let arterial_return = reverse_to_waypoint(&mut game, id, arterial_target);
    assert!(arterial_return <= 5.0, "bike should turn onto arterial: {arterial_return:.1}");
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    assert_eq!(game.mounted_vehicle(), Some(id));
    assert!(
        world.tile_at(Position::new(
            world_to_tile_coord(game.player_pose().position.x),
            world_to_tile_coord(game.player_pose().position.y),
        )).is_some_and(|tile| tile.ground.is_road()),
        "bike should finish on the connected arterial"
    );
    eprintln!(
        "ribbon return: heading {heading_before_turn:.2} -> {:.2}, local {local_return:.1}, collector {collector_return:.1}, arterial {arterial_return:.1}",
        mounted_heading(&game, id)
    );
}
