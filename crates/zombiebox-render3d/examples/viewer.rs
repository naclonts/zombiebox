//! Interactive desktop 3D viewer. Run with `cargo run -p zombiebox-render3d --example viewer --features viewer -- [--birch] [--seed N]`.
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};
use zombiebox_core::{
    ActionGame, ActionInput, CountyFixture, Position, Tile3D, Vec2, Weapon, World,
};
use zombiebox_render3d::{CameraMode, GpuContext, Presentation, SurfaceFrameOutcome};

const TICK: Duration = Duration::from_nanos(1_000_000_000 / 60);
const HELP: &str = "WASD move · arrows aim · Space fire · E interact · Z/X/C/V weapons · R reload · H heal · Shift walk · P pause · 1–5 cameras · drag to orbit · wheel zoom · B Birch · N county · Esc quit";

#[derive(Clone, Copy)]
enum CountyScene {
    Default,
    ResidentialStreet,
    ColonialLot,
    SupportLot,
}

fn county_game(seed: u64, scene: CountyScene) -> Result<ActionGame, String> {
    let fixture = match scene {
        CountyScene::Default => return Ok(ActionGame::county(seed)),
        CountyScene::ResidentialStreet => CountyFixture::ResidentialStreet,
        CountyScene::ColonialLot => CountyFixture::ColonialLot,
        CountyScene::SupportLot => CountyFixture::SupportLot,
    };
    let world = World::with_fixture(seed, fixture);
    let plans = world.fixture_plans();
    let plan = plans
        .first()
        .ok_or("no compatible fixture lot for this seed")?;
    let target = if matches!(scene, CountyScene::ResidentialStreet) {
        let other = plans
            .get(1)
            .ok_or("no compatible reference street for this seed")?;
        let a = plan.structures[0].front_door;
        let b = other.structures[0].front_door;
        let (ix, iy) = plan.frontage.side.inward();
        Position::new((a.x + b.x) / 2 - ix * 8, (a.y + b.y) / 2 - iy * 8)
    } else {
        plan.frontage.access
    };
    let mut game = ActionGame::county_with_world(world);
    game.teleport(Tile3D::from_pos(target, 0))
        .ok_or("fixture access is not a valid teleport destination")?;
    Ok(game)
}

struct View {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: GpuContext,
    presentation: Presentation,
}
struct App {
    view: Option<View>,
    game: ActionGame,
    seed: u64,
    county_scene: CountyScene,
    birch: bool,
    keys: HashSet<KeyCode>,
    once: HashSet<KeyCode>,
    paused: bool,
    mouse_down: bool,
    cursor: Option<(f64, f64)>,
    last: Instant,
    accumulator: Duration,
    title_at: Instant,
    frames: u32,
}
impl App {
    fn new(seed: u64, birch: bool, county_scene: CountyScene) -> Result<Self, String> {
        let game = if birch {
            ActionGame::residential(seed)
        } else {
            county_game(seed, county_scene)?
        };
        Ok(Self {
            view: None,
            game,
            seed,
            county_scene,
            birch,
            keys: HashSet::new(),
            once: HashSet::new(),
            paused: false,
            mouse_down: false,
            cursor: None,
            last: Instant::now(),
            accumulator: Duration::ZERO,
            title_at: Instant::now(),
            frames: 0,
        })
    }
    fn restart(&mut self, birch: bool) {
        self.game = if birch {
            ActionGame::residential(self.seed)
        } else {
            match county_game(self.seed, self.county_scene) {
                Ok(game) => game,
                Err(error) => {
                    eprintln!("Could not restart county: {error}");
                    return;
                }
            }
        };
        self.birch = birch;
        self.paused = false;
        self.accumulator = Duration::ZERO;
        if let Some(v) = &mut self.view {
            v.presentation.reset();
        }
    }
    fn down(&self, k: KeyCode) -> bool {
        self.keys.contains(&k)
    }
    fn tick(&mut self) {
        let axis = |neg, pos| f32::from(self.down(pos)) - f32::from(self.down(neg));
        let movement_stick = Vec2::new(
            axis(KeyCode::KeyA, KeyCode::KeyD),
            axis(KeyCode::KeyW, KeyCode::KeyS),
        );
        let look_stick = Vec2::new(
            axis(KeyCode::ArrowLeft, KeyCode::ArrowRight),
            axis(KeyCode::ArrowUp, KeyCode::ArrowDown),
        );
        let (movement, aim) = {
            let Some(v) = &mut self.view else { return };
            let camera = v.presentation.camera_mut();
            match (self.game.mounted_vehicle(), camera.mode()) {
                (Some(_), _) => (Vec2::ZERO, None),
                (None, CameraMode::Chase | CameraMode::FirstPerson) => {
                    camera.apply_look_input(look_stick, TICK.as_secs_f32());
                    let frame = camera.current_frame();
                    let movement = frame.map_stick_to_world(movement_stick);
                    let aim = Vec2::new(-frame.yaw.cos(), -frame.yaw.sin());
                    (movement, Some(aim))
                }
                (None, _) => {
                    let frame = camera.current_frame();
                    let movement = frame.map_stick_to_world(movement_stick);
                    let aim = frame.map_stick_to_world(look_stick);
                    (movement, Some(aim))
                }
            }
        };
        let selected = [
            (KeyCode::KeyZ, Weapon::Axe),
            (KeyCode::KeyX, Weapon::Pistol),
            (KeyCode::KeyC, Weapon::Shotgun),
            (KeyCode::KeyV, Weapon::RocketLauncher),
        ]
        .into_iter()
        .find_map(|(k, w)| self.once.contains(&k).then_some(w));
        let input = ActionInput {
            movement,
            aim,
            fire: self.down(KeyCode::Space),
            interact: self.once.contains(&KeyCode::KeyE),
            select_weapon: selected,
            reload: self.once.contains(&KeyCode::KeyR),
            use_medkit: self.once.contains(&KeyCode::KeyH),
            quiet_walk: self.down(KeyCode::ShiftLeft) || self.down(KeyCode::ShiftRight),
            ..Default::default()
        };
        self.game.step(input);
        self.once.clear();
        if let Some(v) = &mut self.view {
            v.presentation.observe_tick(&self.game);
        }
    }
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last)
            .min(Duration::from_millis(250));
        self.last = now;
        if !self.paused {
            self.accumulator = (self.accumulator + elapsed).min(TICK * 8);
            while self.accumulator >= TICK {
                self.tick();
                self.accumulator -= TICK;
            }
        } else {
            self.accumulator = Duration::ZERO;
            self.once.clear();
        }
        let Some(v) = &mut self.view else { return };
        let size = v.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        match v.presentation.render_to_surface(
            &v.gpu,
            &v.surface,
            &v.config,
            &self.game,
            elapsed.as_secs_f32(),
        ) {
            Ok(SurfaceFrameOutcome::NeedsReconfigure | SurfaceFrameOutcome::SurfaceLost) => {
                v.surface.configure(&v.gpu.device, &v.config)
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("render error: {e}");
                event_loop.exit();
                return;
            }
        }
        self.frames += 1;
        if now.duration_since(self.title_at) >= Duration::from_secs(1) {
            let h = self.game.hud();
            let s = v.presentation.last_frame_stats();
            let fps = self.frames as f64 / now.duration_since(self.title_at).as_secs_f64();
            v.window.set_title(&format!("Zombiebox {} seed {} | {:?}{} | HP {}/{} | {:?} {}/{} +{} | medkits {} | infected {} | caches {}/{} | {:.0} FPS | chunks {}/{} | {} triangles",if self.birch{"Birch"}else{"County"},self.seed,v.presentation.camera().mode(),if self.paused{" PAUSED"}else{""},h.health,h.max_health,h.equipped,h.active_mag,h.active_mag_cap,h.active_reserve,h.medkits,h.zombie_count,h.caches_found,h.caches_total,fps,s.chunks_drawn,s.chunks_resident,s.triangles_submitted));
            self.title_at = now;
            self.frames = 0;
        }
        v.window.request_redraw();
    }
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Zombiebox 3D")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                )
                .expect("create window"),
        );
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let gpu = pollster::block_on(GpuContext::from_instance_and_surface(
            instance,
            Some(&surface),
        ))
        .expect("GPU context");
        let size = window.inner_size();
        let config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .expect("surface configuration");
        surface.configure(&gpu.device, &config);
        let presentation = Presentation::new(&gpu, config.width, config.height);
        self.view = Some(View {
            window,
            surface,
            config,
            gpu,
            presentation,
        });
        self.last = Instant::now();
        eprintln!("{HELP}");
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(false) => {
                self.keys.clear();
                self.once.clear();
                self.mouse_down = false;
                self.accumulator = Duration::ZERO
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let Some(v) = &mut self.view {
                        v.config.width = size.width;
                        v.config.height = size.height;
                        v.surface.configure(&v.gpu.device, &v.config);
                        v.presentation
                            .camera_mut()
                            .set_viewport(size.width, size.height);
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(k) = event.physical_key {
                    if event.state == ElementState::Released {
                        self.keys.remove(&k);
                    } else if self.keys.insert(k) {
                        self.once.insert(k);
                        match k {
                            KeyCode::Escape => event_loop.exit(),
                            KeyCode::KeyP => {
                                self.paused = !self.paused;
                                self.accumulator = Duration::ZERO
                            }
                            KeyCode::KeyB => self.restart(true),
                            KeyCode::KeyN => self.restart(false),
                            KeyCode::Digit1
                            | KeyCode::Digit2
                            | KeyCode::Digit3
                            | KeyCode::Digit4
                            | KeyCode::Digit5 => {
                                let mode = match k {
                                    KeyCode::Digit1 => CameraMode::Isometric,
                                    KeyCode::Digit2 => CameraMode::Overhead,
                                    KeyCode::Digit3 => CameraMode::Orbit,
                                    KeyCode::Digit4 => CameraMode::Chase,
                                    _ => CameraMode::FirstPerson,
                                };
                                let heading = self.game.player_body().heading;
                                if let Some(v) = &mut self.view {
                                    let camera = v.presentation.camera_mut();
                                    camera.set_mode(mode);
                                    if matches!(mode, CameraMode::Chase | CameraMode::FirstPerson) {
                                        camera.initialize_view_from_heading(heading);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                self.mouse_down = state == ElementState::Pressed;
                self.cursor = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.mouse_down {
                    if let (Some((x, y)), Some(v)) = (self.cursor, &mut self.view) {
                        v.presentation.camera_mut().orbit_rotate(
                            (position.x - x) as f32 * 0.005,
                            (position.y - y) as f32 * 0.005,
                        );
                    }
                }
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if let Some(v) = &mut self.view {
                    v.presentation.camera_mut().orbit_zoom_by(-d * 0.5);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(v) = &self.view {
            v.window.request_redraw();
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut birch = false;
    let mut seed = 42;
    let mut county_scene = CountyScene::Default;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--birch" => birch = true,
            "--residential-street" => county_scene = CountyScene::ResidentialStreet,
            "--colonial-lot" => county_scene = CountyScene::ColonialLot,
            "--support-lot" => county_scene = CountyScene::SupportLot,
            "--seed" => seed = args.next().ok_or("--seed requires an integer")?.parse()?,
            "--help" | "-h" => {
                println!(
                    "Usage: viewer [--birch | --residential-street | --colonial-lot | --support-lot] [--seed N]\n{HELP}"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let mut app = App::new(seed, birch, county_scene)?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_scenes_start_at_their_access_points() {
        for (scene, fixture) in [
            (
                CountyScene::ResidentialStreet,
                CountyFixture::ResidentialStreet,
            ),
            (CountyScene::ColonialLot, CountyFixture::ColonialLot),
            (CountyScene::SupportLot, CountyFixture::SupportLot),
        ] {
            let world = World::with_fixture(42, fixture);
            let plans = world.fixture_plans();
            let expected = if matches!(scene, CountyScene::ResidentialStreet) {
                let a = plans[0].structures[0].front_door;
                let b = plans[1].structures[0].front_door;
                let (ix, iy) = plans[0].frontage.side.inward();
                Position::new((a.x + b.x) / 2 - ix * 8, (a.y + b.y) / 2 - iy * 8)
            } else {
                plans[0].frontage.access
            };
            assert_eq!(
                county_game(42, scene).unwrap().player_tile().pos(),
                expected
            );
        }
    }
}
