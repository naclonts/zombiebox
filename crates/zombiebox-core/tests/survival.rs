use zombiebox_core::{
    CHUNK_SIZE, Command, Direction, District, Event, Game, Ground, Position, WORLD_SIZE, World,
};

#[test]
fn chunk_queries_preserve_geography_and_join_roads() {
    let world = World::new(42);
    for (cx, cy, district) in [
        (8, 16, District::Residential),
        (15, 16, District::City),
        (16, 12, District::Hospital),
        (26, 4, District::MilitaryBase),
        (24, 23, District::Farm),
    ] {
        let p = Position::new(cx * CHUNK_SIZE, cy * CHUNK_SIZE + 8);
        assert_eq!(world.district_at(p), Some(district));
        let chunk = world.chunk(cx, cy).unwrap();
        assert!(chunk[8].iter().all(|tile| tile.ground == Ground::Road));
        assert_eq!(chunk[8][0], world.tile_at(p).unwrap());
        assert_eq!(
            world.tile_at(Position::new(p.x - 1, p.y)).unwrap().ground,
            Ground::Road
        );
        assert_eq!(chunk.iter().flatten().filter(|tile| tile.cache).count(), 1);
    }
    assert!(world.chunk(i32::MAX, 0).is_none());
    assert!(world.tile_at(Position::new(0, WORLD_SIZE)).is_none());
    assert!(world.tile_at(Position::new(-1, 0)).is_none());
}

#[test]
fn wake_cache_is_single_use_and_an_attack_produces_one_blast() {
    let mut game = Game::new(17);
    let wake = game.hud().player;
    assert!(!game.hud().armed);
    assert!(game.has_cache(wake));
    game.step(Command::Scavenge);
    assert!(game.hud().armed);
    assert!(!game.has_cache(wake));
    assert_eq!(game.events(), &[Event::Scavenged { at: wake }]);
    game.step(Command::Scavenge);
    assert!(game.events().is_empty());
    assert_eq!(game.hud().supplies, 3);
    game.step(Command::Attack);
    assert!(matches!(game.events(), [Event::Explosion { .. }]));
    assert_eq!(game.hud().zombie_count, 1);
    assert_eq!(game.hud().supplies, 2);
    game.step(Command::Move(Direction::West));
    assert_eq!(game.hud().player, Position::new(wake.x - 1, wake.y));
    game.step(Command::Attack);
    assert_eq!(game.hud().zombie_count, 0);
}

#[test]
fn run_is_replayable_and_death_ends_the_run() {
    let mut game = Game::new(123);
    let mut replay = Game::new(123);
    assert_ne!(game.world().chunk(8, 16), World::new(124).chunk(8, 16));
    for command in [
        Command::Move(Direction::West),
        Command::Move(Direction::East),
    ]
    .into_iter()
    .chain(std::iter::repeat_n(Command::Wait, 128))
    {
        game.step(command);
        replay.step(command);
        assert_eq!(game.snapshot(), replay.snapshot());
        assert_eq!(game.events(), replay.events());
        if game.hud().health == 0 {
            assert!(game.events().contains(&Event::Died));
            break;
        }
    }
    let at_death = game.hud();
    assert_eq!(at_death.health, 0);
    game.step(Command::Scavenge);
    assert_eq!(game.hud(), at_death);
    assert!(game.events().is_empty());
}
