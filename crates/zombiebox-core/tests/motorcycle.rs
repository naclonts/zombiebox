use zombiebox_core::{
    ActionGame, ActionInput, ControlMode, InteractPrompt, MotorcycleInput, RideInput, Tile3D, Vec2,
    VehicleId, VehicleView,
};

fn motorcycle_views(game: &ActionGame) -> Vec<(VehicleId, VehicleView)> {
    game.vehicle_views().collect()
}

fn starter_bike(game: &ActionGame) -> (VehicleId, Vec2) {
    motorcycle_views(game)
        .into_iter()
        .find_map(|(id, view)| match view {
            VehicleView::Motorcycle(bike) if id.0 & 0xff == 0 => {
                Some((id, bike.body.position.xy()))
            }
            _ => None,
        })
        .expect("starter motorcycle in the active window")
}

fn ride(throttle: f32, brake: f32, steer: f32, lean: f32) -> ActionInput {
    ActionInput {
        ride: Some(RideInput::Motorcycle(MotorcycleInput {
            steer,
            throttle,
            brake,
            lean,
        })),
        ..ActionInput::default()
    }
}

#[test]
fn seeded_roster_and_starter_identity_replay() {
    let a = ActionGame::county(42);
    let b = ActionGame::county(42);
    let roster_a: Vec<_> = motorcycle_views(&a)
        .into_iter()
        .map(|(id, view)| (id, view))
        .collect();
    let roster_b: Vec<_> = motorcycle_views(&b)
        .into_iter()
        .map(|(id, view)| (id, view))
        .collect();
    assert_eq!(roster_a, roster_b);
    assert!(!roster_a.is_empty());
    assert!(roster_a.iter().any(|(id, _)| id.0 & 0xff == 0));

    let (_, starter) = starter_bike(&a);
    let spawn = a.player_pose().position;
    assert!(starter.distance(spawn) <= 13.0);
    assert_eq!(a.control_mode(), ControlMode::OnFoot);
    assert!(a.mounted_vehicle().is_none());
}

#[test]
fn headless_mount_coast_brake_dismount_and_remount_same_id() {
    let mut game = ActionGame::county(42);
    let (id, bike_pos) = starter_bike(&game);
    let bike_tile = Tile3D::from_vec2(bike_pos, 0);
    assert!(
        game.teleport(bike_tile).is_some(),
        "teleporting beside the starter bike should succeed"
    );

    // The mount prompt is offered before consuming the pulse.
    assert!(
        matches!(
            game.hud().interact_prompt,
            InteractPrompt::Mount | InteractPrompt::RightBike
        ) || game.player_pose().position.distance(bike_pos) <= 1.35,
        "starter bike should be within mounting reach"
    );
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    assert_eq!(game.mounted_vehicle(), Some(id));
    let mounted_hud = game.hud().mounted.expect("mounted HUD");
    assert_eq!(mounted_hud.control_mode, ControlMode::Motorcycle);
    assert_eq!(mounted_hud.condition_pct, 100);

    let start = game.player_pose().position;
    for _ in 0..70 {
        game.step(ride(0.85, 0.0, 0.0, 0.0));
    }
    let after_throttle = game.player_pose().position;
    assert!(
        after_throttle.distance(start) > 2.0,
        "bike should ride forward: start {start:?}, now {after_throttle:?}"
    );
    assert!(game.hud().mounted.unwrap().speed_mph_x10 > 10);

    let coast_start = game.player_pose().position;
    for _ in 0..20 {
        game.step(ActionInput::default());
    }
    let coast_end = game.player_pose().position;
    assert!(
        coast_end.distance(coast_start) > 0.2,
        "coasting should carry momentum"
    );

    for _ in 0..240 {
        let speed = game.hud().mounted.unwrap().speed_mph_x10;
        if speed == 0 {
            break;
        }
        game.step(ride(0.0, 1.0, 0.0, 0.0));
    }
    assert_eq!(
        game.hud().mounted.unwrap().speed_mph_x10,
        0,
        "braking should stop the bike"
    );
    assert_eq!(game.hud().interact_prompt, InteractPrompt::Dismount);

    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::OnFoot);
    assert!(game.mounted_vehicle().is_none());
    let dismounted_at = game.player_pose().position;
    assert!(dismounted_at.distance(game.player_pose().position) < 1e-6);

    // The same stable ID can be mounted again from the standing position.
    assert!(game.hud().interact_prompt == InteractPrompt::Mount);
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    assert_eq!(game.mounted_vehicle(), Some(id));
}

#[test]
fn steering_and_lean_turn_the_mounted_bike() {
    let mut game = ActionGame::county(42);
    let (_, bike_pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(bike_pos, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);

    for _ in 0..20 {
        game.step(ride(0.6, 0.0, 0.0, 0.0));
    }
    let heading_before = game.player_body().heading;
    for _ in 0..45 {
        game.step(ride(0.45, 0.0, 0.85, 0.6));
    }
    let heading_after = game.player_body().heading;
    assert!(
        (heading_after - heading_before).abs() > 0.2,
        "steering should turn the bike: {heading_before} -> {heading_after}"
    );
    let bike = match game
        .vehicle_views()
        .find(|(id, _)| *id == game.mounted_vehicle().unwrap())
        .unwrap()
        .1
    {
        VehicleView::Motorcycle(bike) => bike,
    };
    assert!(bike.rider_lean.abs() > 0.1);
}

#[test]
fn mounted_teleport_is_rejected_and_on_foot_prompts_still_work() {
    let mut game = ActionGame::county(42);
    let (_, bike_pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(bike_pos, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    let target = game.player_tile();
    assert!(game.teleport(target).is_none());

    // Riding does not leave stale weapon fire on the following tick.
    game.step(ride(0.0, 1.0, 0.0, 0.0));
    game.step(ActionInput {
        fire: true,
        ..ActionInput::default()
    });
    assert!(game.events().iter().all(|event| !matches!(
        event,
        zombiebox_core::ActionEvent::ShotFired { .. }
            | zombiebox_core::ActionEvent::AxeSwing { .. }
    )));
}

#[test]
fn legacy_tick_keeps_motorcycle_interactions_disabled() {
    let mut game = ActionGame::county(42);
    let (_, pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(pos, 0)).is_some());
    game.step_without_vehicle_interactions(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::OnFoot);
}

#[test]
fn legacy_tick_cannot_dismount_an_already_mounted_vehicle() {
    let mut game = ActionGame::county(42);
    let (_, pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(pos, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    game.step_without_vehicle_interactions(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
}

#[test]
fn low_speed_hard_lean_tips_the_bike_and_recovery_expires() {
    let mut game = ActionGame::county(42);
    let (id, pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(pos, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    let mut saw_tip = false;
    for _ in 0..20 {
        game.step(ride(0.0, 0.0, 0.0, 1.0));
        saw_tip |= game.events().iter().any(|e| {
            matches!(
                e,
                zombiebox_core::ActionEvent::VehicleCrash {
                    kind: zombiebox_core::MotorcycleCrashKind::LowSpeedTip,
                    ..
                }
            )
        });
    }
    assert_eq!(game.control_mode(), ControlMode::OnFoot);
    assert!(game.is_recovering_from_vehicle_crash());
    assert!(saw_tip);
    assert!(
        matches!(game.vehicle_views().find(|(got, _)| *got == id).unwrap().1, VehicleView::Motorcycle(b) if b.phase == zombiebox_core::MotorcyclePhase::Tipped)
    );
    assert!(
        game.teleport(Tile3D::new(pos.x as i32 + 30, pos.y as i32, 0))
            .is_none()
    );
    for _ in 0..60 {
        game.step(ActionInput::default());
    }
    assert!(!game.is_recovering_from_vehicle_crash());
    assert_eq!(game.hud().interact_prompt, InteractPrompt::RightBike);
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert!(
        matches!(game.vehicle_views().find(|(got, _)| *got == id).unwrap().1, VehicleView::Motorcycle(b) if b.phase == zombiebox_core::MotorcyclePhase::Parked)
    );
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    for _ in 0..20 {
        game.step(ride(0.0, 0.0, 0.0, 1.0));
    }
    assert!(
        matches!(game.vehicle_views().find(|(got, _)| *got == id).unwrap().1, VehicleView::Motorcycle(b) if b.condition == 90 && b.phase == zombiebox_core::MotorcyclePhase::Tipped)
    );
}

#[test]
fn sharp_steer_at_road_speed_ejects_rider_and_keeps_skidding_bike() {
    let mut game = ActionGame::county(42);
    let (id, pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(pos, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    for _ in 0..220 {
        game.step(ride(1.0, 0.0, 0.0, 0.0));
    }
    assert!(
        game.hud().mounted.unwrap().speed_mph_x10 >= 300,
        "must reach road speed before steering yank"
    );
    let hp = game.hud().health;
    game.step(ride(0.0, 0.0, 1.0, -1.0));
    assert_eq!(game.control_mode(), ControlMode::OnFoot);
    assert!(game.hud().health < hp);
    assert!(game.events().iter().any(|e| matches!(e, zombiebox_core::ActionEvent::VehicleSkidStarted { id: got, .. } if *got == id)));
    let rider_before_tumble = game.player_pose().position;
    game.step(ActionInput::default());
    assert!(
        game.player_pose().position.distance(rider_before_tumble) > 0.05,
        "crash knockback should carry the ejected rider into recovery"
    );
    let before = game.vehicle_views().find(|(got, _)| *got == id).unwrap().1;
    assert!(
        matches!(before, VehicleView::Motorcycle(b) if b.phase == zombiebox_core::MotorcyclePhase::Skidding && b.condition < 100)
    );
    for _ in 0..30 {
        game.step(ActionInput::default());
    }
    let after = game.vehicle_views().find(|(got, _)| *got == id).unwrap().1;
    let (VehicleView::Motorcycle(a), VehicleView::Motorcycle(b)) = (before, after);
    assert_ne!(a.body.position, b.body.position);
}

#[test]
fn lean_is_primary_steering_mechanism_at_high_speed_without_crashing() {
    let mut game = ActionGame::county(42);
    let (_, bike_pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(bike_pos, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    // Accelerate past high-speed threshold (~15+ mph)
    for _ in 0..120 {
        game.step(ride(0.9, 0.0, 0.0, 0.0));
    }
    assert!(
        game.hud().mounted.unwrap().speed_mph_x10 >= 180,
        "must exceed high speed threshold"
    );
    let heading_before = game.player_body().heading;
    for _ in 0..40 {
        game.step(ride(0.6, 0.0, 0.0, 0.8));
    }
    let heading_after = game.player_body().heading;
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    assert_eq!(game.hud().mounted.unwrap().condition_pct, 100);
    assert!(
        (heading_after - heading_before).abs() > 0.15,
        "lean alone must steer the motorcycle at high speed: {heading_before} -> {heading_after}"
    );
}

#[test]
fn full_handlebar_steer_at_low_speed_turns_cleanly_without_crashing() {
    let mut game = ActionGame::county(42);
    let (_, bike_pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(bike_pos, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    // Accelerate to ~5 mph (speed_mph_x10 around 50)
    for _ in 0..35 {
        game.step(ride(0.65, 0.0, 0.0, 0.0));
    }
    let speed = game.hud().mounted.unwrap().speed_mph_x10;
    assert!(speed >= 30 && speed <= 75, "speed in low regime: {speed}");
    let heading_before = game.player_body().heading;
    for _ in 0..25 {
        game.step(ride(0.35, 0.0, 1.0, 0.0));
    }
    let heading_after = game.player_body().heading;
    assert_eq!(game.control_mode(), ControlMode::Motorcycle);
    assert_eq!(game.hud().mounted.unwrap().condition_pct, 100);
    assert!(
        (heading_after - heading_before).abs() > 0.3,
        "handlebar steering must turn the bike sharply at low speed"
    );
}

#[test]
fn hard_lean_at_two_mph_tips_over() {
    let mut game = ActionGame::county(42);
    let (_, bike_pos) = starter_bike(&game);
    assert!(game.teleport(Tile3D::from_vec2(bike_pos, 0)).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    // Reach ~2 mph (speed_mph_x10 around 20)
    for _ in 0..20 {
        game.step(ride(0.4, 0.0, 0.0, 0.0));
    }
    let speed = game.hud().mounted.unwrap().speed_mph_x10;
    assert!(speed >= 15 && speed <= 30, "speed around 2 mph: {speed}");
    let mut saw_tip = false;
    for _ in 0..25 {
        game.step(ride(0.1, 0.0, 0.0, 1.0));
        saw_tip |= game.events().iter().any(|e| {
            matches!(
                e,
                zombiebox_core::ActionEvent::VehicleCrash {
                    kind: zombiebox_core::MotorcycleCrashKind::LowSpeedTip,
                    ..
                }
            )
        });
    }
    assert!(saw_tip, "hard lean at ~2 mph must tip the bike over");
    assert_eq!(game.control_mode(), ControlMode::OnFoot);
}
