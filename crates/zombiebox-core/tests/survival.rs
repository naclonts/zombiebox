use zombiebox_core::{Command, Direction, Event, Game, Position};

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
