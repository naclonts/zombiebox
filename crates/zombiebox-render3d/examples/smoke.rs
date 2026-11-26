//! Headless `wgpu` smoke renderer CLI writing a 640x360 PNG.

use std::path::PathBuf;

use zombiebox_render3d::gpu::{GpuContext, render_smoke_cube, write_rgba8_png};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_path = std::env::args().nth(1).map_or_else(
        || PathBuf::from("/tmp/zombiebox-render3d-smoke.png"),
        PathBuf::from,
    );

    let ctx = GpuContext::new_headless()?;
    let width = 640;
    let height = 360;
    let image = render_smoke_cube(&ctx, width, height, 0.0)?;
    write_rgba8_png(&out_path, &image)?;

    println!(
        "Rendered {}x{} smoke cube using adapter '{}' ({:?}, {:?}) -> {}",
        width,
        height,
        ctx.adapter_info.name,
        ctx.adapter_info.backend,
        ctx.adapter_info.device_type,
        out_path.display()
    );
    Ok(())
}
