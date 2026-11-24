use zombiebox_core::{Command, Direction, Game};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seed = std::env::args()
        .nth(1)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(42);
    let mut game = Game::new(seed);
    let wake = game.hud().player;
    println!(
        "Woke at ({}, {}) in {:?} (seed {seed})",
        wake.x,
        wake.y,
        game.world().district_at(wake).unwrap()
    );
    for command in [
        Command::Scavenge,
        Command::Attack,
        Command::Move(Direction::West),
        Command::Attack,
    ] {
        game.step(command);
        let hud = game.hud();
        println!(
            "{:2} {command:?}  supplies={}  zombies={}  {:?}",
            hud.tick,
            hud.supplies,
            hud.zombie_count,
            game.events()
        );
    }
    Ok(())
}
