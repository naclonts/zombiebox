//! Android surface integration. The renderer retains its own ANativeWindow reference.
use super::*;
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::{
    ffi::{CString, c_char, c_void},
    ptr::NonNull,
};
use zombiebox_render3d::{CameraMode, GpuContext, Presentation, SurfaceFrameOutcome};

pub(super) struct SurfaceState {
    // Drop the surface before the context and before the caller releases its window.
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    context: GpuContext,
    pub(super) presentation: Presentation,
    _window: WindowRef,
}

unsafe extern "C" {
    fn ANativeWindow_acquire(window: *mut c_void);
    fn ANativeWindow_release(window: *mut c_void);
    fn __android_log_write(priority: i32, tag: *const c_char, text: *const c_char) -> i32;
}
struct WindowRef(NonNull<c_void>);
impl Drop for WindowRef {
    fn drop(&mut self) {
        unsafe { ANativeWindow_release(self.0.as_ptr()) }
    }
}

fn log_surface_error(error: impl std::fmt::Display) {
    if let Ok(message) = CString::new(error.to_string()) {
        // ANDROID_LOG_ERROR
        unsafe { __android_log_write(6, c"Zombiebox3D".as_ptr(), message.as_ptr()) };
    }
}

fn configure(
    surface: &wgpu::Surface<'_>,
    context: &GpuContext,
    config: &wgpu::SurfaceConfiguration,
) -> bool {
    use wgpu::ErrorFilter;
    let validation = context.device.push_error_scope(ErrorFilter::Validation);
    let internal = context.device.push_error_scope(ErrorFilter::Internal);
    let memory = context.device.push_error_scope(ErrorFilter::OutOfMemory);
    surface.configure(&context.device, config);
    let errors = [
        pollster::block_on(memory.pop()),
        pollster::block_on(internal.pop()),
        pollster::block_on(validation.pop()),
    ];
    let mut configured = true;
    for error in errors.into_iter().flatten() {
        log_surface_error(error);
        configured = false;
    }
    configured
}

/// The window must be a live ANativeWindow and the run exclusively owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_attach_window(
    run: *mut ZbAction,
    window: *mut c_void,
    width: u32,
    height: u32,
) -> bool {
    let (Some(run), Some(window)) = (unsafe { run.as_mut() }, NonNull::new(window)) else {
        return false;
    };
    if width == 0 || height == 0 {
        return false;
    }
    unsafe { ANativeWindow_acquire(window.as_ptr()) };
    let window_ref = WindowRef(window);
    run.gpu = None;
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
        raw_window_handle: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(window)),
    };
    let surface = match unsafe { instance.create_surface_unsafe(target) } {
        Ok(surface) => surface,
        Err(error) => {
            log_surface_error(error);
            return false;
        }
    };
    let context = match pollster::block_on(GpuContext::from_instance_and_surface(
        instance,
        Some(&surface),
    )) {
        Ok(context) => context,
        Err(error) => {
            log_surface_error(error);
            return false;
        }
    };
    let Some(config) = surface.get_default_config(&context.adapter, width, height) else {
        return false;
    };
    if !configure(&surface, &context, &config) {
        return false;
    }
    let mut presentation = Presentation::new(&context, width, height);
    presentation.set_adaptive_resolution(true);
    run.gpu = Some(SurfaceState {
        surface,
        config,
        context,
        presentation,
        _window: window_ref,
    });
    run.reset_clock_and_latches();
    true
}

/// The run and output pointers must be live, aligned and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_frame_timing(
    run: *const ZbAction,
    out: *mut zombiebox_render3d::FrameTiming,
) -> bool {
    let Some(gpu) = (unsafe { run.as_ref() }).and_then(|r| r.gpu.as_ref()) else {
        return false;
    };
    let Some(timing) = gpu.presentation.latest_frame_timing() else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    unsafe { out.write(timing) };
    true
}

/// The run and output pointers must be live, aligned and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_frame_percentiles(
    run: *const ZbAction,
    out: *mut zombiebox_render3d::FramePercentiles,
) -> bool {
    let Some(gpu) = (unsafe { run.as_ref() }).and_then(|r| r.gpu.as_ref()) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    unsafe { out.write(gpu.presentation.frame_percentiles()) };
    true
}

/// The run must be live and exclusively owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_detach_window(run: *mut ZbAction) {
    if let Some(run) = unsafe { run.as_mut() } {
        run.gpu = None;
        run.reset_clock_and_latches();
    }
}

/// The run must be live and exclusively owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_resize_window(
    run: *mut ZbAction,
    width: u32,
    height: u32,
) -> bool {
    let Some(gpu) = (unsafe { run.as_mut() }).and_then(|r| r.gpu.as_mut()) else {
        return false;
    };
    if width == 0 || height == 0 {
        return false;
    }
    let mut config = gpu.config.clone();
    config.width = width;
    config.height = height;
    if !configure(&gpu.surface, &gpu.context, &config) {
        return false;
    }
    gpu.config = config;
    true
}

/// The run must be live and exclusively owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_camera_mode(run: *mut ZbAction, mode: u8) -> bool {
    let Some(gpu) = (unsafe { run.as_mut() }).and_then(|r| r.gpu.as_mut()) else {
        return false;
    };
    let mode = match mode {
        0 => CameraMode::Isometric,
        1 => CameraMode::Overhead,
        2 => CameraMode::Orbit,
        3 => CameraMode::Chase,
        4 => CameraMode::FirstPerson,
        _ => return false,
    };
    gpu.presentation.camera_mut().set_mode(mode);
    true
}

/// Pointers must be live, aligned, disjoint and exclusively accessible for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_frame_surface(
    run: *mut ZbAction,
    frame_time_ns: u64,
    input: *const ZbActionFrameInput,
    out: *mut ZbActionFrameOutput,
) -> bool {
    let (Some(run), Some(input)) = (unsafe { run.as_mut() }, unsafe { input.as_ref() }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let Ok(weapon) = parse_select_weapon(input.select_weapon) else {
        return false;
    };
    let Some(gpu) = run.gpu.as_mut() else {
        return false;
    };
    gpu.presentation
        .camera_mut()
        .set_look_ahead(input.action_flags & ZB_INPUT_LOOK_AHEAD != 0);
    let frame = gpu.presentation.camera().current_frame();
    let map = |x: f32, y: f32| frame.map_stick_to_world(Vec2::new(x, y));
    let movement = map(input.move_x, input.move_y);
    let aim = map(input.aim_x, input.aim_y);
    let previous = run.last_frame;
    let audio = run.advance_v2(
        frame_time_ns,
        movement,
        (aim != Vec2::ZERO).then_some(aim),
        input.action_flags,
        weapon,
    );
    let dt = previous.map_or(0.0, |t| {
        frame_time_ns.saturating_sub(t).min(MAX_FRAME_NS) as f32 / 1_000_000_000.0
    });
    let gpu = run.gpu.as_mut().expect("checked above");
    let rendered =
        gpu.presentation
            .render_to_surface(&gpu.context, &gpu.surface, &gpu.config, &run.game, dt);
    if matches!(
        rendered,
        Ok(SurfaceFrameOutcome::NeedsReconfigure
            | SurfaceFrameOutcome::SurfaceLost
            | SurfaceFrameOutcome::Presented {
                suboptimal: true,
                ..
            })
    ) {
        configure(&gpu.surface, &gpu.context, &gpu.config);
    }
    unsafe { out.write(run.output_v2(audio)) };
    rendered.is_ok()
}
