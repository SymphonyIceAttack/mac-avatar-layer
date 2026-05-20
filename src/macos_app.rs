#![cfg(target_os = "macos")]
#![allow(unsafe_op_in_unsafe_fn)]

use crate::audio_input::MicrophoneInput;
use crate::config::AppConfig;
use crate::cubism;
use crate::live2d_model::Live2dModel;
#[cfg(feature = "metal-renderer")]
use crate::metal_renderer::MetalRenderer;
use crate::motion::MotionInput;
#[cfg(all(feature = "cubism-core", not(feature = "metal-renderer")))]
use crate::software_renderer::SoftwareRenderer;
use std::ffi::{CString, c_char, c_double, c_long, c_ulong, c_void};
#[cfg(not(feature = "metal-renderer"))]
use std::path::Path;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

type Id = *mut c_void;
type Class = *mut c_void;
type Sel = *mut c_void;
type Bool = i8;
type NSInteger = c_long;
type NSUInteger = c_ulong;
type CGFloat = c_double;

const YES: Bool = 1;
const NO: Bool = 0;
const NIL: Id = ptr::null_mut();

const NS_APPLICATION_ACTIVATION_POLICY_ACCESSORY: NSInteger = 1;
const NS_BACKING_STORE_BUFFERED: NSUInteger = 2;
const NS_BORDERLESS_WINDOW_MASK: NSUInteger = 0;
const NS_FLOATING_WINDOW_LEVEL: NSInteger = 3;
const NS_EVENT_MASK_ANY: NSUInteger = NSUInteger::MAX;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: NSUInteger = 1 << 0;
const NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY: NSUInteger = 1 << 4;
const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY: NSUInteger = 1 << 8;
const NS_ACTIVITY_AUTOMATIC_TERMINATION_DISABLED: NSUInteger = 1 << 15;
const NS_ACTIVITY_USER_INITIATED_ALLOWING_IDLE_SYSTEM_SLEEP: NSUInteger = 0x00ff_ffff;
const TARGET_FPS: f64 = 60.0;
#[cfg(feature = "metal-renderer")]
const AVATAR_HORIZONTAL_MARGIN: CGFloat = 36.0;
#[cfg(feature = "metal-renderer")]
const AVATAR_BOTTOM_RESERVED: CGFloat = 92.0;
#[cfg(feature = "metal-renderer")]
const AVATAR_TOP_RESERVED: CGFloat = 100.0;

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: CGFloat,
    y: CGFloat,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize {
    width: CGFloat,
    height: CGFloat,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Class;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

#[cfg(all(feature = "cubism-core", not(feature = "metal-renderer")))]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> Id;
    fn CGColorSpaceRelease(space: Id);
    fn CGDataProviderCreateWithCFData(data: Id) -> Id;
    fn CGDataProviderRelease(provider: Id);
    fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        color_space: Id,
        bitmap_info: u32,
        provider: Id,
        decode: *const CGFloat,
        should_interpolate: Bool,
        intent: u32,
    ) -> Id;
    fn CGImageRelease(image: Id);
}

pub fn run(model_path: &str) -> Result<(), String> {
    unsafe {
        let app = msg_id(class("NSApplication")?, "sharedApplication");
        msg_void_id(
            app,
            "setActivationPolicy:",
            NS_APPLICATION_ACTIVATION_POLICY_ACCESSORY,
        );
        let _activity_token = prevent_app_nap()?;
        let config = AppConfig::load()?;
        let model = Live2dModel::load(model_path)?;
        println!("Loaded {}", model.summary());
        println!("Model root: {}", model.root_dir.display());
        println!("Moc: {}", model.moc.display());
        for texture in &model.textures {
            println!("Texture: {}", texture.display());
        }
        let mut cubism_runtime = cubism::load_runtime(&model)?;
        let cubism_summary = cubism_runtime.info().summary();
        println!("{cubism_summary}");
        log_offscreen_status(cubism_runtime.info());
        log_cubism_preview(&cubism_runtime);
        let window = create_avatar_window()?;
        println!("renderer_event=window_created kind=avatar");
        let root_layer = create_root_layer(window)?;
        #[allow(unused_mut)]
        let mut renderer_diagnostics = RendererDiagnostics::from_config(&config.renderer)
            .with_offscreen_count(cubism_runtime.info().offscreen_count);
        #[cfg(feature = "metal-renderer")]
        let mut metal_renderer = {
            let mut renderer = MetalRenderer::load(&model, &config.renderer)?;
            let probe = renderer.render_probe(&cubism_runtime);
            renderer_diagnostics.apply_metal_probe(&probe);
            println!(
                "Metal renderer: device '{}' textures {} drawables {} triangles {} additive {} multiply {} extended_blend {} masked {} queue {}",
                probe.device_name,
                probe.texture_count,
                probe.drawable_count,
                probe.triangle_count,
                probe.additive_count,
                probe.multiplicative_count,
                probe.extended_blend_count,
                probe.masked_count,
                probe.has_command_queue
            );
            if probe.extended_blend_count > 0 {
                println!(
                    "Metal renderer: first-pass extended Cubism blend shader enabled for {count} objects",
                    count = probe.extended_blend_count
                );
            }
            install_metal_layer(root_layer, &mut renderer)?;
            renderer
        };
        #[cfg(not(feature = "metal-renderer"))]
        let avatar_layer = create_avatar_layer(&model)?;
        let diagnostics_layer = create_diagnostics_layer()?;
        #[cfg(not(feature = "metal-renderer"))]
        msg_void_id(root_layer, "addSublayer:", avatar_layer);
        if config.diagnostics.show {
            msg_void_id(root_layer, "addSublayer:", diagnostics_layer);
        }

        msg_void_id(window, "makeKeyAndOrderFront:", NIL);
        msg_void(window, "orderFrontRegardless");
        msg_void_id(app, "activateIgnoringOtherApps:", YES);

        let run_loop_mode = ns_string("kCFRunLoopDefaultMode")?;
        let mut frame_clock = FrameClock::new(TARGET_FPS);
        let mut diagnostics = Diagnostics::new(
            frame_clock.frame_duration(),
            model.summary(),
            cubism_summary,
            renderer_diagnostics,
        );
        #[cfg(all(feature = "cubism-core", not(feature = "metal-renderer")))]
        let mut software_renderer = SoftwareRenderer::load(&model)?;
        let mut motion_controller = crate::motion::MotionController::new(&model, &config);
        let microphone = MicrophoneInput::from_config(&config.input.microphone);
        let started_at = Instant::now();
        let mut last_frame_at = started_at;
        let mut lifecycle_monitor = AppLifecycleMonitor::new();
        lifecycle_monitor.poll(app, window, started_at);

        loop {
            drain_pending_events(app, run_loop_mode);
            lifecycle_monitor.poll(app, window, started_at);
            begin_immediate_layer_update();
            let now = Instant::now();
            motion_controller.apply(
                &mut cubism_runtime,
                now.saturating_duration_since(last_frame_at),
                &MotionInput {
                    pointer: normalized_mouse_position(),
                    mouth_level: microphone.as_ref().map(MicrophoneInput::level),
                },
            );
            last_frame_at = now;
            #[cfg(feature = "metal-renderer")]
            sync_metal_layer_geometry(window, root_layer, &mut metal_renderer)?;
            #[cfg(feature = "metal-renderer")]
            metal_renderer.render(&cubism_runtime)?;
            #[cfg(all(feature = "cubism-core", not(feature = "metal-renderer")))]
            {
                let rgba = software_renderer.render(&cubism_runtime);
                set_layer_bitmap(avatar_layer, rgba, 512, 512)?;
            }
            #[cfg(not(feature = "cubism-core"))]
            draw_avatar_frame(avatar_layer, started_at.elapsed().as_secs_f64())?;
            diagnostics.record_frame(diagnostics_layer, started_at)?;
            commit_layer_update();
            frame_clock.sleep_until_next_frame();
        }
    }
}

fn log_offscreen_status(info: &cubism::CubismRuntimeInfo) {
    let Some(count) = info.offscreen_count else {
        return;
    };
    if count <= 0 {
        return;
    }

    #[cfg(feature = "metal-renderer")]
    println!(
        "Metal renderer: first-pass Cubism offscreen render path enabled for {count} offscreens"
    );
    #[cfg(not(feature = "metal-renderer"))]
    eprintln!(
        "Renderer warning: model reports {count} Cubism offscreen drawables, but this renderer path does not implement offscreen passes; affected parts may render differently."
    );
}

fn log_cubism_preview(runtime: &cubism::CubismModelRuntime) {
    let parameters = runtime.parameters();
    if !parameters.is_empty() {
        println!("Cubism parameters: {}", parameters.len());
        for parameter in parameters.iter().take(8) {
            println!(
                "  param {} value {:.3} default {:.3} range [{:.3}, {:.3}]",
                parameter.id, parameter.value, parameter.default, parameter.min, parameter.max
            );
        }
        for parameter in parameters.iter().filter(|parameter| {
            matches!(parameter.id.as_str(), "ParamMouthForm" | "ParamMouthOpenY")
        }) {
            println!(
                "  mouth param {} value {:.3} default {:.3} range [{:.3}, {:.3}]",
                parameter.id, parameter.value, parameter.default, parameter.min, parameter.max
            );
        }
    }

    let drawables = runtime.drawables();
    if !drawables.is_empty() {
        println!("Cubism drawables: {}", drawables.len());
        for drawable in drawables.iter().take(8) {
            println!(
                "  drawable #{} {} part {}({}) blend {} tex {} vertices {} indices {} opacity {:.3} draw {} render {} masks {} flags visible={} double_sided={} additive={} multiply={} inverted_mask={}",
                drawable.index,
                drawable.id,
                drawable.parent_part_id.as_deref().unwrap_or("-"),
                drawable.parent_part_index,
                drawable.blend_mode.description(),
                drawable.texture_index,
                drawable.vertex_count,
                drawable.index_count,
                drawable.opacity,
                drawable.draw_order,
                drawable.render_order,
                drawable.masks.len(),
                drawable.flags.visible,
                drawable.flags.double_sided,
                drawable.flags.blend_additive,
                drawable.flags.blend_multiplicative,
                drawable.flags.inverted_mask
            );
        }

        for drawable in drawables.iter().filter(|drawable| {
            matches!(drawable.parent_part_id.as_deref(), Some("Part6" | "Part9"))
        }) {
            println!(
                "  mouth drawable #{} {} part {}({}) tex {} opacity {:.3} draw {} render {} masks {:?} visible={} inverted_mask={}",
                drawable.index,
                drawable.id,
                drawable.parent_part_id.as_deref().unwrap_or("-"),
                drawable.parent_part_index,
                drawable.texture_index,
                drawable.opacity,
                drawable.draw_order,
                drawable.render_order,
                drawable.masks,
                drawable.flags.visible,
                drawable.flags.inverted_mask
            );
        }

        if let Some(frame) = runtime.drawable_frame_by_index(0) {
            println!(
                "First drawable frame: positions {} uvs {} indices {}",
                frame.positions.len(),
                frame.uvs.len(),
                frame.indices.len()
            );
        }
    }

    let offscreens = runtime.offscreens();
    if !offscreens.is_empty() {
        let parts = runtime.parts();
        println!("Cubism offscreens: {}", offscreens.len());
        for offscreen in offscreens.iter().take(8) {
            let owner = parts
                .get(offscreen.owner_part_index.max(0) as usize)
                .map(|part| format!("{}({})", part.id, part.index))
                .unwrap_or_else(|| format!("-({})", offscreen.owner_part_index));
            println!(
                "  offscreen #{} owner {} render {} blend {} opacity {:.3} masks {:?} inverted_mask={}",
                offscreen.index,
                owner,
                offscreen.render_order,
                offscreen.blend_mode.description(),
                offscreen.opacity,
                offscreen.masks,
                offscreen.flags.inverted_mask
            );
        }
    }
}

#[cfg(all(feature = "cubism-core", not(feature = "metal-renderer")))]
unsafe fn set_layer_bitmap(
    layer: Id,
    rgba: &[u8],
    width: usize,
    height: usize,
) -> Result<(), String> {
    let expected_len = width * height * 4;
    if rgba.len() != expected_len {
        return Err(format!(
            "RGBA buffer has {} bytes, expected {}",
            rgba.len(),
            expected_len
        ));
    }

    let data = msg_id_bytes(
        class("NSData")?,
        "dataWithBytes:length:",
        rgba.as_ptr().cast::<c_void>(),
        rgba.len(),
    );
    if data.is_null() {
        return Err("NSData dataWithBytes:length: returned nil".to_string());
    }

    let color_space = CGColorSpaceCreateDeviceRGB();
    if color_space.is_null() {
        return Err("CGColorSpaceCreateDeviceRGB returned null".to_string());
    }

    let provider = CGDataProviderCreateWithCFData(data);
    if provider.is_null() {
        CGColorSpaceRelease(color_space);
        return Err("CGDataProviderCreateWithCFData returned null".to_string());
    }

    let image = CGImageCreate(
        width,
        height,
        8,
        32,
        width * 4,
        color_space,
        0x2002,
        provider,
        ptr::null(),
        0,
        0,
    );

    CGDataProviderRelease(provider);
    CGColorSpaceRelease(color_space);

    if image.is_null() {
        return Err("CGImageCreate returned null".to_string());
    }

    msg_void_id(layer, "setContents:", image);
    CGImageRelease(image);
    Ok(())
}

unsafe fn prevent_app_nap() -> Result<Id, String> {
    let options = NS_ACTIVITY_USER_INITIATED_ALLOWING_IDLE_SYSTEM_SLEEP
        | NS_ACTIVITY_AUTOMATIC_TERMINATION_DISABLED;
    let reason = ns_string("Keep vtube-studio-rs avatar rendering while switching Spaces")?;
    let process_info = msg_id(class("NSProcessInfo")?, "processInfo");
    if process_info.is_null() {
        return Err("NSProcessInfo processInfo returned nil".to_string());
    }

    let token = msg_id_ulong_id(
        process_info,
        "beginActivityWithOptions:reason:",
        options,
        reason,
    );
    if token.is_null() {
        return Err("beginActivityWithOptions returned nil".to_string());
    }

    println!("renderer_event=app_nap_guard_started");
    Ok(token)
}

unsafe fn create_avatar_window() -> Result<Id, String> {
    let rect = NSRect {
        origin: NSPoint { x: 100.0, y: 140.0 },
        size: NSSize {
            width: 360.0,
            height: 480.0,
        },
    };

    let window = msg_id(class("NSWindow")?, "alloc");
    let window = msg_id_rect_ulong_ulong_bool(
        window,
        "initWithContentRect:styleMask:backing:defer:",
        rect,
        NS_BORDERLESS_WINDOW_MASK,
        NS_BACKING_STORE_BUFFERED,
        NO,
    );

    if window.is_null() {
        return Err("NSWindow allocation returned nil".to_string());
    }

    let behavior = NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
        | NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY
        | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY;

    msg_void_bool(window, "setOpaque:", NO);
    msg_void_bool(window, "setMovableByWindowBackground:", YES);
    msg_void_bool(window, "setReleasedWhenClosed:", NO);
    msg_void_int(window, "setLevel:", NS_FLOATING_WINDOW_LEVEL);
    msg_void_ulong(window, "setCollectionBehavior:", behavior);
    println!(
        "renderer_event=window_configured level={} collection_behavior={}",
        NS_FLOATING_WINDOW_LEVEL, behavior
    );

    let clear = ns_color(0.0, 0.0, 0.0, 0.0)?;
    msg_void_id(window, "setBackgroundColor:", clear);

    Ok(window)
}

unsafe fn create_root_layer(window: Id) -> Result<Id, String> {
    let content_view = msg_id(window, "contentView");
    if content_view.is_null() {
        return Err("window contentView returned nil".to_string());
    }

    msg_void_bool(content_view, "setWantsLayer:", YES);
    let layer = msg_id(content_view, "layer");
    if layer.is_null() {
        return Err("contentView layer returned nil".to_string());
    }

    msg_void_bool(layer, "setNeedsDisplayOnBoundsChange:", YES);
    msg_void_bool(layer, "setAllowsEdgeAntialiasing:", YES);
    let color = ns_color(0.04, 0.05, 0.07, 0.72)?;
    let cg_color = msg_id(color, "CGColor");
    msg_void_id(layer, "setBackgroundColor:", cg_color);
    msg_void_double(layer, "setCornerRadius:", 24.0);

    Ok(layer)
}

#[cfg(not(feature = "metal-renderer"))]
unsafe fn create_avatar_layer(model: &Live2dModel) -> Result<Id, String> {
    let layer = msg_id(class("CALayer")?, "layer");
    if layer.is_null() {
        return Err("CALayer allocation returned nil".to_string());
    }

    let frame = NSRect {
        origin: NSPoint { x: 76.0, y: 72.0 },
        size: NSSize {
            width: 208.0,
            height: 288.0,
        },
    };
    msg_void_rect(layer, "setFrame:", frame);
    msg_void_double(layer, "setCornerRadius:", 104.0);
    msg_void_bool(layer, "setMasksToBounds:", YES);
    msg_void_bool(layer, "setAllowsEdgeAntialiasing:", YES);
    msg_void_id(layer, "setContentsGravity:", ns_string("resizeAspectFill")?);

    if let Some(texture) = model.primary_texture() {
        set_layer_image(layer, texture)?;
    }

    Ok(layer)
}

#[cfg(feature = "metal-renderer")]
unsafe fn install_metal_layer(root_layer: Id, renderer: &mut MetalRenderer) -> Result<(), String> {
    let layer = renderer.layer_ptr();
    if layer.is_null() {
        return Err("CAMetalLayer allocation returned nil".to_string());
    }

    let frame = avatar_frame_for_bounds(msg_rect(root_layer, "bounds"));
    renderer.set_drawable_size(frame.size.width, frame.size.height);
    msg_void_rect(layer, "setFrame:", frame);
    msg_void_double(layer, "setZPosition:", 1.0);
    msg_void_bool(layer, "setAllowsEdgeAntialiasing:", YES);
    msg_void_id(root_layer, "addSublayer:", layer);
    Ok(())
}

#[cfg(feature = "metal-renderer")]
unsafe fn sync_metal_layer_geometry(
    window: Id,
    root_layer: Id,
    renderer: &mut MetalRenderer,
) -> Result<(), String> {
    let layer = renderer.layer_ptr();
    if layer.is_null() {
        return Err("CAMetalLayer pointer became nil".to_string());
    }

    let contents_scale = msg_double(window, "backingScaleFactor").max(1.0);
    let frame = avatar_frame_for_bounds(msg_rect(root_layer, "bounds"));
    msg_void_rect(layer, "setFrame:", frame);
    msg_void_double(layer, "setContentsScale:", contents_scale);
    renderer.set_contents_scale(contents_scale);
    renderer.set_drawable_size(frame.size.width, frame.size.height);
    Ok(())
}

#[cfg(feature = "metal-renderer")]
fn avatar_frame_for_bounds(bounds: NSRect) -> NSRect {
    let available_width = (bounds.size.width - AVATAR_HORIZONTAL_MARGIN * 2.0).max(1.0);
    let available_height =
        (bounds.size.height - AVATAR_BOTTOM_RESERVED - AVATAR_TOP_RESERVED).max(1.0);
    let size = available_width.min(available_height).max(1.0);
    let x = bounds.origin.x + ((bounds.size.width - size) * 0.5).max(0.0);
    let y = bounds.origin.y + AVATAR_BOTTOM_RESERVED.min((bounds.size.height - size).max(0.0));

    NSRect {
        origin: NSPoint { x, y },
        size: NSSize {
            width: size,
            height: size,
        },
    }
}

unsafe fn create_diagnostics_layer() -> Result<Id, String> {
    let layer = msg_id(class("CATextLayer")?, "layer");
    if layer.is_null() {
        return Err("CATextLayer allocation returned nil".to_string());
    }

    let frame = NSRect {
        origin: NSPoint { x: 18.0, y: 18.0 },
        size: NSSize {
            width: 324.0,
            height: 148.0,
        },
    };

    let text_color = ns_color(0.92, 0.97, 1.0, 0.92)?;
    let text_cg_color = msg_id(text_color, "CGColor");
    msg_void_rect(layer, "setFrame:", frame);
    msg_void_id(layer, "setForegroundColor:", text_cg_color);
    msg_void_double(layer, "setFontSize:", 13.0);
    msg_void_double(layer, "setContentsScale:", 2.0);
    msg_void_double(layer, "setZPosition:", 10.0);
    msg_void_bool(layer, "setWrapped:", YES);
    set_layer_text(
        layer,
        "Model: loading\nCubism Core: loading\nFPS: warming up\nFrame delta: warming up\nSlow frames: 0\nFrames: 0\nApp Nap guard: active",
    )?;

    Ok(layer)
}

#[cfg(not(feature = "metal-renderer"))]
unsafe fn set_layer_image(layer: Id, path: &Path) -> Result<(), String> {
    let path_string = path
        .to_str()
        .ok_or_else(|| format!("Texture path is not valid UTF-8: {}", path.display()))?;
    let ns_path = ns_string(path_string)?;
    let image = msg_id_id(
        msg_id(class("NSImage")?, "alloc"),
        "initWithContentsOfFile:",
        ns_path,
    );
    if image.is_null() {
        return Err(format!("Failed to load texture image: {}", path.display()));
    }

    msg_void_id(layer, "setContents:", image);
    Ok(())
}

unsafe fn begin_immediate_layer_update() {
    let transaction = class("CATransaction").expect("CATransaction must exist on macOS");
    msg_void(transaction, "begin");
    msg_void_bool(transaction, "setDisableActions:", YES);
}

unsafe fn commit_layer_update() {
    let transaction = class("CATransaction").expect("CATransaction must exist on macOS");
    msg_void(transaction, "commit");
}

unsafe fn normalized_mouse_position() -> Option<[f32; 2]> {
    let mouse = msg_point(class("NSEvent").ok()?, "mouseLocation");
    let screen = msg_id(class("NSScreen").ok()?, "mainScreen");
    if screen.is_null() {
        return None;
    }

    let frame = msg_rect(screen, "frame");
    if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
        return None;
    }

    let x = ((mouse.x - frame.origin.x) / frame.size.width * 2.0 - 1.0) as f32;
    let y = ((mouse.y - frame.origin.y) / frame.size.height * 2.0 - 1.0) as f32;
    Some([x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)])
}

#[cfg(not(feature = "cubism-core"))]
unsafe fn draw_avatar_frame(layer: Id, elapsed_seconds: f64) -> Result<(), String> {
    let breathe = (elapsed_seconds * 2.2).sin() * 0.5 + 0.5;
    let red = 0.30 + breathe * 0.12;
    let green = 0.72 + breathe * 0.10;
    let blue = 0.86 + breathe * 0.08;
    let color = ns_color(red, green, blue, 0.94)?;
    let cg_color = msg_id(color, "CGColor");
    msg_void_id(layer, "setBackgroundColor:", cg_color);

    let y = 216.0 + (elapsed_seconds * 1.7).sin() * 8.0;
    msg_void_point(layer, "setPosition:", NSPoint { x: 180.0, y });
    Ok(())
}

unsafe fn set_layer_text(layer: Id, text: &str) -> Result<(), String> {
    let text = ns_string(text)?;
    msg_void_id(layer, "setString:", text);
    Ok(())
}

unsafe fn drain_pending_events(app: Id, run_loop_mode: Id) {
    loop {
        let event = msg_id_mask_date_mode_bool(
            app,
            "nextEventMatchingMask:untilDate:inMode:dequeue:",
            NS_EVENT_MASK_ANY,
            NIL,
            run_loop_mode,
            YES,
        );

        if event.is_null() {
            break;
        }

        msg_void_id(app, "sendEvent:", event);
        msg_void(app, "updateWindows");
    }
}

struct FrameClock {
    frame_duration: Duration,
    next_frame: Instant,
}

impl FrameClock {
    fn new(fps: f64) -> Self {
        Self {
            frame_duration: Duration::from_secs_f64(1.0 / fps),
            next_frame: Instant::now(),
        }
    }

    fn frame_duration(&self) -> Duration {
        self.frame_duration
    }

    fn sleep_until_next_frame(&mut self) {
        self.next_frame += self.frame_duration;
        let now = Instant::now();
        if self.next_frame > now {
            thread::sleep(self.next_frame - now);
        } else {
            self.next_frame = now;
        }
    }
}

struct Diagnostics {
    target_frame_duration: Duration,
    model_summary: String,
    cubism_summary: String,
    renderer_summary: String,
    total_frames: u64,
    slow_frames: u64,
    frames_since_report: u64,
    intervals_since_report: u64,
    interval_sum_since_report: Duration,
    worst_interval_since_report: Duration,
    last_frame_at: Option<Instant>,
    last_report: Instant,
}

struct AppLifecycleMonitor {
    last_poll: Instant,
    app_active: Option<bool>,
    window_visible: Option<bool>,
    window_occlusion_state: Option<NSUInteger>,
}

impl AppLifecycleMonitor {
    const POLL_INTERVAL: Duration = Duration::from_millis(500);

    fn new() -> Self {
        Self {
            last_poll: Instant::now() - Self::POLL_INTERVAL,
            app_active: None,
            window_visible: None,
            window_occlusion_state: None,
        }
    }

    unsafe fn poll(&mut self, app: Id, window: Id, started_at: Instant) {
        let now = Instant::now();
        if now.duration_since(self.last_poll) < Self::POLL_INTERVAL {
            return;
        }
        self.last_poll = now;
        let uptime = now.duration_since(started_at).as_secs_f64();

        let app_active = msg_bool(app, "isActive");
        if self.app_active != Some(app_active) {
            println!(
                "renderer_event=app_active_changed active={} uptime_s={uptime:.1}",
                app_active
            );
            self.app_active = Some(app_active);
        }

        let window_visible = msg_bool(window, "isVisible");
        if self.window_visible != Some(window_visible) {
            println!(
                "renderer_event=window_visible_changed visible={} uptime_s={uptime:.1}",
                window_visible
            );
            self.window_visible = Some(window_visible);
        }

        let occlusion_state = msg_ulong(window, "occlusionState");
        if self.window_occlusion_state != Some(occlusion_state) {
            println!(
                "renderer_event=window_occlusion_changed state={} uptime_s={uptime:.1}",
                occlusion_state
            );
            self.window_occlusion_state = Some(occlusion_state);
        }
    }
}

#[derive(Debug, Clone)]
struct RendererDiagnostics {
    mask_mode: String,
    debug_texture_mode: String,
    atlas_mipmaps: bool,
    hidden_count: usize,
    only_count: usize,
    highlight_count: usize,
    offscreen_count: usize,
    extended_blend_count: usize,
}

impl RendererDiagnostics {
    fn from_config(config: &crate::config::RendererConfig) -> Self {
        let mask_mode = if config.disable_masks {
            "disabled"
        } else if config.high_precision_masks {
            "high_precision"
        } else {
            "shared"
        }
        .to_string();
        Self {
            mask_mode,
            debug_texture_mode: config
                .debug_texture_mode
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("none")
                .to_string(),
            atlas_mipmaps: config.atlas_mipmaps,
            hidden_count: config.hidden_drawables.len() + config.hidden_parts.len(),
            only_count: config.only_drawables.len() + config.only_parts.len(),
            highlight_count: config.highlight_drawables.len() + config.highlight_parts.len(),
            offscreen_count: 0,
            extended_blend_count: 0,
        }
    }

    #[cfg(feature = "metal-renderer")]
    fn apply_metal_probe(&mut self, probe: &crate::metal_renderer::MetalRenderProbe) {
        self.extended_blend_count = probe.extended_blend_count;
    }

    fn with_offscreen_count(mut self, offscreen_count: Option<i32>) -> Self {
        self.offscreen_count = offscreen_count.unwrap_or(0).max(0) as usize;
        if self.offscreen_count > 0 && self.mask_mode == "high_precision" {
            self.mask_mode = "shared(offscreen)".to_string();
        }
        self
    }

    fn summary(&self) -> String {
        format!(
            "Renderer: mask {} | offscreen {} | ext blend {} | debug {} | mipmaps {} | filters h/o/hi {}/{}/{}",
            self.mask_mode,
            self.offscreen_count,
            self.extended_blend_count,
            self.debug_texture_mode,
            if self.atlas_mipmaps { "on" } else { "off" },
            self.hidden_count,
            self.only_count,
            self.highlight_count
        )
    }
}

impl Diagnostics {
    fn new(
        target_frame_duration: Duration,
        model_summary: String,
        cubism_summary: String,
        renderer_diagnostics: RendererDiagnostics,
    ) -> Self {
        let now = Instant::now();
        Self {
            target_frame_duration,
            model_summary,
            cubism_summary,
            renderer_summary: renderer_diagnostics.summary(),
            total_frames: 0,
            slow_frames: 0,
            frames_since_report: 0,
            intervals_since_report: 0,
            interval_sum_since_report: Duration::ZERO,
            worst_interval_since_report: Duration::ZERO,
            last_frame_at: None,
            last_report: now,
        }
    }

    unsafe fn record_frame(&mut self, layer: Id, started_at: Instant) -> Result<(), String> {
        self.total_frames += 1;
        self.frames_since_report += 1;

        let now = Instant::now();
        if let Some(last_frame_at) = self.last_frame_at {
            let interval = now.duration_since(last_frame_at);
            self.intervals_since_report += 1;
            self.interval_sum_since_report += interval;
            self.worst_interval_since_report = self.worst_interval_since_report.max(interval);

            if interval > self.target_frame_duration.mul_f64(1.5) {
                self.slow_frames += 1;
            }

            if interval >= Duration::from_millis(250) {
                println!(
                    "renderer_event=long_frame_gap gap_ms={:.1} uptime_s={:.1}",
                    duration_ms(interval),
                    now.duration_since(started_at).as_secs_f64(),
                );
            }
            if interval >= Duration::from_secs(5) {
                println!(
                    "renderer_event=display_wake_inferred gap_ms={:.1} uptime_s={:.1}",
                    duration_ms(interval),
                    now.duration_since(started_at).as_secs_f64(),
                );
            }
        }
        self.last_frame_at = Some(now);

        let report_interval = now.duration_since(self.last_report);
        if report_interval < Duration::from_millis(500) {
            return Ok(());
        }

        let fps = self.frames_since_report as f64 / report_interval.as_secs_f64();
        let uptime = now.duration_since(started_at).as_secs_f64();
        let avg_interval = if self.intervals_since_report == 0 {
            Duration::ZERO
        } else {
            self.interval_sum_since_report / self.intervals_since_report as u32
        };
        let text = format!(
            "Model: {}\n{}\n{}\nFPS: {:>5.1} / {:.0}\nFrame delta: avg {:>5.1} ms, max {:>5.1} ms\nBudget: {:>5.1} ms\nSlow frames: {}\nFrames: {}\nUptime: {:>6.1}s\nApp Nap guard: active",
            self.model_summary,
            self.cubism_summary,
            self.renderer_summary,
            fps,
            TARGET_FPS,
            duration_ms(avg_interval),
            duration_ms(self.worst_interval_since_report),
            duration_ms(self.target_frame_duration),
            self.slow_frames,
            self.total_frames,
            uptime
        );
        set_layer_text(layer, &text)?;

        self.frames_since_report = 0;
        self.intervals_since_report = 0;
        self.interval_sum_since_report = Duration::ZERO;
        self.worst_interval_since_report = Duration::ZERO;
        self.last_report = now;
        Ok(())
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

unsafe fn class(name: &str) -> Result<Class, String> {
    let name = CString::new(name).map_err(|error| error.to_string())?;
    let class = objc_getClass(name.as_ptr());
    if class.is_null() {
        Err(format!(
            "Objective-C class not found: {}",
            name.to_string_lossy()
        ))
    } else {
        Ok(class)
    }
}

unsafe fn selector(name: &str) -> Sel {
    let name = CString::new(name).expect("selector names must not contain NUL bytes");
    sel_registerName(name.as_ptr())
}

unsafe fn ns_string(value: &str) -> Result<Id, String> {
    let value = CString::new(value).map_err(|error| error.to_string())?;
    let string = msg_id(class("NSString")?, "alloc");
    Ok(msg_id_cstr(string, "initWithUTF8String:", value.as_ptr()))
}

unsafe fn ns_color(red: f64, green: f64, blue: f64, alpha: f64) -> Result<Id, String> {
    Ok(msg_id_double_double_double_double(
        class("NSColor")?,
        "colorWithCalibratedRed:green:blue:alpha:",
        red,
        green,
        blue,
        alpha,
    ))
}

unsafe fn msg_id(receiver: Id, selector_name: &str) -> Id {
    let function: extern "C" fn(Id, Sel) -> Id = std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name))
}

unsafe fn msg_bool(receiver: Id, selector_name: &str) -> bool {
    let function: extern "C" fn(Id, Sel) -> Bool = std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name)) != NO
}

unsafe fn msg_ulong(receiver: Id, selector_name: &str) -> NSUInteger {
    let function: extern "C" fn(Id, Sel) -> NSUInteger =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name))
}

#[cfg(feature = "metal-renderer")]
unsafe fn msg_double(receiver: Id, selector_name: &str) -> CGFloat {
    let function: extern "C" fn(Id, Sel) -> CGFloat =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name))
}

unsafe fn msg_void(receiver: Id, selector_name: &str) {
    let function: extern "C" fn(Id, Sel) = std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name));
}

unsafe fn msg_void_id<T>(receiver: Id, selector_name: &str, value: T) {
    let function: extern "C" fn(Id, Sel, T) = std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), value);
}

unsafe fn msg_void_bool(receiver: Id, selector_name: &str, value: Bool) {
    msg_void_id(receiver, selector_name, value);
}

unsafe fn msg_void_int(receiver: Id, selector_name: &str, value: NSInteger) {
    msg_void_id(receiver, selector_name, value);
}

unsafe fn msg_void_ulong(receiver: Id, selector_name: &str, value: NSUInteger) {
    msg_void_id(receiver, selector_name, value);
}

unsafe fn msg_void_double(receiver: Id, selector_name: &str, value: f64) {
    msg_void_id(receiver, selector_name, value);
}

unsafe fn msg_void_rect(receiver: Id, selector_name: &str, rect: NSRect) {
    msg_void_id(receiver, selector_name, rect);
}

unsafe fn msg_point(receiver: Id, selector_name: &str) -> NSPoint {
    let function: extern "C" fn(Id, Sel) -> NSPoint =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name))
}

unsafe fn msg_rect(receiver: Id, selector_name: &str) -> NSRect {
    let function: extern "C" fn(Id, Sel) -> NSRect = std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name))
}

#[cfg(not(feature = "cubism-core"))]
unsafe fn msg_void_point(receiver: Id, selector_name: &str, point: NSPoint) {
    msg_void_id(receiver, selector_name, point);
}

unsafe fn msg_id_cstr(receiver: Id, selector_name: &str, value: *const c_char) -> Id {
    let function: extern "C" fn(Id, Sel, *const c_char) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), value)
}

#[cfg(all(feature = "cubism-core", not(feature = "metal-renderer")))]
unsafe fn msg_id_bytes(receiver: Id, selector_name: &str, bytes: *const c_void, len: usize) -> Id {
    let function: extern "C" fn(Id, Sel, *const c_void, usize) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), bytes, len)
}

#[cfg(not(feature = "metal-renderer"))]
unsafe fn msg_id_id(receiver: Id, selector_name: &str, value: Id) -> Id {
    let function: extern "C" fn(Id, Sel, Id) -> Id = std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), value)
}

unsafe fn msg_id_ulong_id(
    receiver: Id,
    selector_name: &str,
    options: NSUInteger,
    reason: Id,
) -> Id {
    let function: extern "C" fn(Id, Sel, NSUInteger, Id) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), options, reason)
}

unsafe fn msg_id_rect_ulong_ulong_bool(
    receiver: Id,
    selector_name: &str,
    rect: NSRect,
    style: NSUInteger,
    backing: NSUInteger,
    defer: Bool,
) -> Id {
    let function: extern "C" fn(Id, Sel, NSRect, NSUInteger, NSUInteger, Bool) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(
        receiver,
        selector(selector_name),
        rect,
        style,
        backing,
        defer,
    )
}

unsafe fn msg_id_mask_date_mode_bool(
    receiver: Id,
    selector_name: &str,
    mask: NSUInteger,
    date: Id,
    mode: Id,
    dequeue: Bool,
) -> Id {
    let function: extern "C" fn(Id, Sel, NSUInteger, Id, Id, Bool) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), mask, date, mode, dequeue)
}

unsafe fn msg_id_double_double_double_double(
    receiver: Id,
    selector_name: &str,
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
) -> Id {
    let function: extern "C" fn(Id, Sel, f64, f64, f64, f64) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), red, green, blue, alpha)
}

#[cfg(all(test, feature = "metal-renderer"))]
mod tests {
    use super::{NSPoint, NSRect, NSSize, avatar_frame_for_bounds};

    #[test]
    fn avatar_frame_matches_default_window_layout() {
        let frame = avatar_frame_for_bounds(NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: 360.0,
                height: 480.0,
            },
        });

        assert_eq!(frame.origin.x, 36.0);
        assert_eq!(frame.origin.y, 92.0);
        assert_eq!(frame.size.width, 288.0);
        assert_eq!(frame.size.height, 288.0);
    }

    #[test]
    fn avatar_frame_stays_positive_for_small_windows() {
        let frame = avatar_frame_for_bounds(NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: 120.0,
                height: 120.0,
            },
        });

        assert!(frame.origin.x >= 0.0);
        assert!(frame.origin.y >= 0.0);
        assert!(frame.size.width >= 1.0);
        assert_eq!(frame.size.width, frame.size.height);
    }
}
