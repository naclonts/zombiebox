//! Write a standalone PPM frame for quick host-side visual inspection.
use std::fs;
use zombiebox_core::ActionGame;
use zombiebox_render::Renderer;

fn main() -> std::io::Result<()> {
    let mut game = ActionGame::residential(42);
    game.step(zombiebox_core::ActionInput::default());
    let mut renderer = Renderer::new(640, 360);
    let width = renderer.width();
    let height = renderer.height();
    let rgba = renderer.render(&game, 0);
    let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in rgba.chunks_exact(4) {
        ppm.extend_from_slice(&pixel[..3]);
    }
    fs::write("/tmp/zombiebox-preview.ppm", ppm)
}
