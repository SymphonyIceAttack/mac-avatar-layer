#![cfg(target_os = "macos")]
#![allow(unsafe_op_in_unsafe_fn)]

use crate::audio_input::MicrophoneInput;
use crate::camera_input::{CameraInput, CameraStatus};
use crate::config::{AppConfig, AppRuntimeConfig, CameraConfig, MicrophoneConfig, MouseConfig};
use crate::cubism;
use crate::live2d_model::Live2dModel;
#[cfg(feature = "metal-renderer")]
use crate::metal_renderer::MetalRenderer;
use crate::motion::MotionInput;
#[cfg(all(feature = "cubism-core", not(feature = "metal-renderer")))]
use crate::software_renderer::SoftwareRenderer;
use std::ffi::{CString, c_char, c_double, c_long, c_ulong, c_void};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
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
const NS_NONACTIVATING_PANEL_MASK: NSUInteger = 1 << 7;
const NS_EVENT_MASK_ANY: NSUInteger = NSUInteger::MAX;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: NSUInteger = 1 << 0;
const NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY: NSUInteger = 1 << 4;
const NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE: NSUInteger = 1 << 6;
const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY: NSUInteger = 1 << 8;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_APPLICATIONS: NSUInteger = 1 << 18;
const NS_WINDOW_OCCLUSION_STATE_VISIBLE: NSUInteger = 1 << 1;
const CG_SCREEN_SAVER_WINDOW_LEVEL_KEY: i32 = 13;
const CG_MAXIMUM_WINDOW_LEVEL_KEY: i32 = 14;
const CG_OVERLAY_WINDOW_LEVEL_KEY: i32 = 15;
const NS_ACTIVITY_AUTOMATIC_TERMINATION_DISABLED: NSUInteger = 1 << 15;
const NS_ACTIVITY_USER_INITIATED_ALLOWING_IDLE_SYSTEM_SLEEP: NSUInteger = 0x00ff_ffff;
const NS_CONTROL_STATE_VALUE_OFF: NSInteger = 0;
const NS_CONTROL_STATE_VALUE_ON: NSInteger = 1;
const NS_VARIABLE_STATUS_ITEM_LENGTH: CGFloat = -1.0;
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
    fn objc_allocateClassPair(superclass: Class, name: *const c_char, extra_bytes: usize) -> Class;
    fn objc_registerClassPair(cls: Class);
    fn class_addMethod(cls: Class, name: Sel, imp: *const c_void, types: *const c_char) -> Bool;
    fn objc_msgSend();
}

static MENU_COMMANDS: AtomicU32 = AtomicU32::new(0);
static MENU_SELECTED_EXPRESSION_INDEX: AtomicI32 = AtomicI32::new(EXPRESSION_INDEX_UNCHANGED);
static MENU_SELECTED_MODEL_INDEX: AtomicI32 = AtomicI32::new(MODEL_INDEX_UNCHANGED);

const MENU_TOGGLE_DIAGNOSTICS: u32 = 1 << 0;
const MENU_TOGGLE_MOUSE: u32 = 1 << 1;
const MENU_TOGGLE_MICROPHONE: u32 = 1 << 2;
const MENU_TOGGLE_CAMERA: u32 = 1 << 3;
const MENU_OPEN_ACTIVE_CONFIG: u32 = 1 << 4;
const MENU_SELECT_EXPRESSION: u32 = 1 << 5;
const MENU_SELECT_MOUSE_PRESET: u32 = 1 << 6;
const MENU_SELECT_MOUTH_PRESET: u32 = 1 << 7;
const MENU_SELECT_CAMERA_PRESET: u32 = 1 << 8;
const MENU_SELECT_MODEL: u32 = 1 << 9;
const MODEL_INDEX_UNCHANGED: i32 = -1;
const EXPRESSION_INDEX_UNCHANGED: i32 = -2;
const EXPRESSION_INDEX_NONE: i32 = -1;
const INPUT_PRESET_UNCHANGED: i32 = -1;

static MENU_SELECTED_MOUSE_PRESET: AtomicI32 = AtomicI32::new(INPUT_PRESET_UNCHANGED);
static MENU_SELECTED_MOUTH_PRESET: AtomicI32 = AtomicI32::new(INPUT_PRESET_UNCHANGED);
static MENU_SELECTED_CAMERA_PRESET: AtomicI32 = AtomicI32::new(INPUT_PRESET_UNCHANGED);

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
}

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

pub fn run(model_path: &str, config: AppConfig) -> Result<(), String> {
    unsafe {
        let app = msg_id(class("NSApplication")?, "sharedApplication");
        msg_void_id(
            app,
            "setActivationPolicy:",
            NS_APPLICATION_ACTIVATION_POLICY_ACCESSORY,
        );
        let _activity_token = prevent_app_nap()?;
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
        let window = create_avatar_window(&config.app)?;
        println!("renderer_event=window_created kind=avatar");
        let root_layer = create_root_layer(window)?;
        #[allow(unused_mut)]
        let mut renderer_diagnostics = RendererDiagnostics::from_config(&config.renderer)
            .with_offscreen_count(cubism_runtime.info().offscreen_count);
        #[cfg(feature = "metal-renderer")]
        let mut metal_renderer = {
            let mut renderer =
                MetalRenderer::load(&model, &config.renderer, config.app.runtime_profile)?;
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
        msg_void_id(root_layer, "addSublayer:", diagnostics_layer);
        let mut diagnostics_visible = config.diagnostics.show;
        msg_void_bool(
            diagnostics_layer,
            "setHidden:",
            bool_to_objc(!diagnostics_visible),
        );
        let mut selected_expression_index =
            selected_expression_index(&model, config.motion.expression.as_deref());
        let mut camera_input = CameraInput::from_config(&config.input.camera);
        let mut camera_enabled = camera_runtime_active(camera_input.status());
        let mut mouse_enabled = config.input.mouse.enabled;
        let mut microphone_enabled = config.input.microphone.enabled;
        let settings_menu = install_settings_menu(
            app,
            &model,
            model_path,
            &config,
            camera_input.status_label(),
            &renderer_diagnostics,
            RuntimeControlState {
                diagnostics_visible,
                mouse_enabled,
                microphone_enabled,
                camera_enabled,
                selected_expression_index,
                mouse_preset: InputPreset::Normal,
                mouth_preset: InputPreset::Normal,
                camera_preset: InputPreset::Normal,
            },
        )?;

        msg_void(window, "orderFrontRegardless");

        let event_pump = EventPump::new()?;
        let mut frame_clock = FrameClock::new(TARGET_FPS);
        let mut diagnostics = Diagnostics::new(
            frame_clock.frame_duration(),
            model.summary(),
            cubism_summary,
            renderer_diagnostics,
            camera_debug_summary(
                camera_input.status(),
                camera_input.latest_sample(),
                camera_input.diagnostic(),
            )
            .overlay_text,
        );
        #[cfg(all(feature = "cubism-core", not(feature = "metal-renderer")))]
        let mut software_renderer = SoftwareRenderer::load(&model)?;
        let mut motion_controller = crate::motion::MotionController::new(&model, &config);
        let mut microphone = MicrophoneInput::from_config(&config.input.microphone);
        let mut mouse_preset = InputPreset::Normal;
        let mut mouth_preset = InputPreset::Normal;
        let mut camera_preset = InputPreset::Normal;
        let mut last_camera_menu_text = format!("Camera Tracking: {}", camera_input.status_label());
        let started_at = Instant::now();
        let mut last_frame_at = started_at;
        let mut lifecycle_monitor = AppLifecycleMonitor::new();
        lifecycle_monitor.poll(app, window, &config.app, started_at);

        loop {
            event_pump.drain_pending_events(app);
            handle_settings_menu_commands(
                diagnostics_layer,
                &settings_menu,
                &mut diagnostics_visible,
                &mut mouse_enabled,
                &mut microphone_enabled,
                &mut camera_enabled,
                &mut selected_expression_index,
                &mut mouse_preset,
                &mut mouth_preset,
                &mut camera_preset,
                &config,
                &model,
                &mut motion_controller,
                &mut microphone,
                &mut camera_input,
            )?;
            lifecycle_monitor.poll(app, window, &config.app, started_at);
            begin_immediate_layer_update();
            let now = Instant::now();
            let camera_sample = camera_input.latest_sample();
            let camera_status = camera_input.status();
            motion_controller.apply(
                &mut cubism_runtime,
                now.saturating_duration_since(last_frame_at),
                &MotionInput {
                    pointer: normalized_mouse_position(window, &config.input.mouse),
                    mouth_level: microphone.as_ref().map(MicrophoneInput::level),
                    camera: camera_sample,
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
            let camera_summary =
                camera_debug_summary(camera_status, camera_sample, camera_input.diagnostic());
            diagnostics.set_camera_summary(camera_summary.overlay_text);
            update_camera_menu_status(
                &settings_menu,
                &camera_summary.menu_text,
                &mut last_camera_menu_text,
            )?;
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

unsafe fn create_avatar_window(app_config: &AppRuntimeConfig) -> Result<Id, String> {
    let window_size = avatar_window_size(app_config);
    let rect = NSRect {
        origin: NSPoint { x: 100.0, y: 140.0 },
        size: window_size,
    };

    let window = msg_id(class("NSPanel")?, "alloc");
    let window = msg_id_rect_ulong_ulong_bool(
        window,
        "initWithContentRect:styleMask:backing:defer:",
        rect,
        avatar_window_style_mask(),
        NS_BACKING_STORE_BUFFERED,
        NO,
    );

    if window.is_null() {
        return Err("NSPanel allocation returned nil".to_string());
    }

    msg_void_bool(window, "setOpaque:", NO);
    msg_void_bool(window, "setMovableByWindowBackground:", YES);
    msg_void_bool(window, "setReleasedWhenClosed:", NO);
    msg_void_bool(window, "setCanHide:", NO);
    msg_void_bool(window, "setFloatingPanel:", YES);
    msg_void_bool(window, "setHidesOnDeactivate:", NO);
    msg_void_bool(window, "setWorksWhenModal:", YES);
    msg_void_bool(window, "setBecomesKeyOnlyIfNeeded:", YES);
    msg_void_bool(window, "setExcludedFromWindowsMenu:", YES);
    apply_avatar_window_space_policy(window, app_config);
    println!(
        "renderer_event=window_configured kind=nonactivating_panel level={} level_name={} level_key={} width={:.1} height={:.1} style_mask={} collection_behavior={}",
        avatar_window_level(app_config),
        avatar_window_level_name(&app_config.window_level),
        avatar_window_level_key(&app_config.window_level),
        window_size.width,
        window_size.height,
        avatar_window_style_mask(),
        avatar_window_collection_behavior()
    );

    let clear = ns_color(0.0, 0.0, 0.0, 0.0)?;
    msg_void_id(window, "setBackgroundColor:", clear);

    Ok(window)
}

fn avatar_window_style_mask() -> NSUInteger {
    NS_BORDERLESS_WINDOW_MASK | NS_NONACTIVATING_PANEL_MASK
}

fn avatar_window_size(app_config: &AppRuntimeConfig) -> NSSize {
    NSSize {
        width: valid_window_dimension(app_config.window_width, 360.0),
        height: valid_window_dimension(app_config.window_height, 480.0),
    }
}

fn valid_window_dimension(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value >= 96.0 {
        value.min(2400.0)
    } else {
        fallback
    }
}

unsafe fn avatar_window_level(app_config: &AppRuntimeConfig) -> NSInteger {
    CGWindowLevelForKey(avatar_window_level_key(&app_config.window_level)) as NSInteger
}

fn avatar_window_level_key(configured: &str) -> i32 {
    match configured.trim().to_ascii_lowercase().as_str() {
        "maximum" | "max" => CG_MAXIMUM_WINDOW_LEVEL_KEY,
        "overlay" => CG_OVERLAY_WINDOW_LEVEL_KEY,
        "screen_saver" | "screensaver" | "screen-saver" | "" => CG_SCREEN_SAVER_WINDOW_LEVEL_KEY,
        _ => CG_SCREEN_SAVER_WINDOW_LEVEL_KEY,
    }
}

fn avatar_window_level_name(configured: &str) -> &'static str {
    match configured.trim().to_ascii_lowercase().as_str() {
        "screen_saver" | "screensaver" | "screen-saver" => "screen_saver",
        "maximum" | "max" => "maximum",
        "overlay" => "overlay",
        "" => "screen_saver",
        _ => "screen_saver",
    }
}

fn avatar_window_collection_behavior() -> NSUInteger {
    // `transient` asks AppKit to hide the panel in Mission Control and made
    // Space swipe transitions produce disappear/double-image frames in testing.
    NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
        | NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_APPLICATIONS
        | NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY
        | NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE
        | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY
}

unsafe fn apply_avatar_window_space_policy(window: Id, app_config: &AppRuntimeConfig) {
    msg_void_int(window, "setLevel:", avatar_window_level(app_config));
    msg_void_ulong(
        window,
        "setCollectionBehavior:",
        avatar_window_collection_behavior(),
    );
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
    let color = ns_color(0.0, 0.0, 0.0, 0.0)?;
    let cg_color = msg_id(color, "CGColor");
    msg_void_id(layer, "setBackgroundColor:", cg_color);
    msg_void_bool(layer, "setOpaque:", NO);
    msg_void_double(layer, "setCornerRadius:", 0.0);

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
            width: 660.0,
            height: 286.0,
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
        "Model: loading\nCubism Core: loading\nRenderer: loading\nCamera: loading\nFPS: warming up\nFrame delta: warming up\nSlow frames: 0\nFrames: 0\nApp Nap guard: active",
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

unsafe fn normalized_mouse_position(window: Id, config: &MouseConfig) -> Option<[f32; 2]> {
    let mouse = msg_point(class("NSEvent").ok()?, "mouseLocation");
    let coordinate_space = config.coordinate_space.trim();
    if coordinate_space.eq_ignore_ascii_case("window") {
        let frame = msg_rect(window, "frame");
        return normalized_point_in_rect(mouse, frame);
    }

    let screen = msg_id(class("NSScreen").ok()?, "mainScreen");
    if screen.is_null() {
        return None;
    }

    normalized_point_in_rect(mouse, msg_rect(screen, "frame"))
}

fn normalized_point_in_rect(point: NSPoint, frame: NSRect) -> Option<[f32; 2]> {
    if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
        return None;
    }

    let x = ((point.x - frame.origin.x) / frame.size.width * 2.0 - 1.0) as f32;
    let y = ((point.y - frame.origin.y) / frame.size.height * 2.0 - 1.0) as f32;
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

#[derive(Clone, Copy)]
struct RuntimeControlState {
    diagnostics_visible: bool,
    mouse_enabled: bool,
    microphone_enabled: bool,
    camera_enabled: bool,
    selected_expression_index: Option<usize>,
    mouse_preset: InputPreset,
    mouth_preset: InputPreset,
    camera_preset: InputPreset,
}

struct SettingsMenu {
    _controller: Id,
    _status_item: Id,
    diagnostics_item: Id,
    mouse_item: Id,
    microphone_item: Id,
    camera_item: Id,
    model_items: Vec<Id>,
    model_entries: Vec<ModelMenuEntry>,
    expression_items: Vec<Id>,
    mouse_preset_items: Vec<Id>,
    mouth_preset_items: Vec<Id>,
    camera_preset_items: Vec<Id>,
}

#[derive(Clone, Debug)]
struct ModelMenuEntry {
    title: String,
    path: String,
    current: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputPreset {
    Soft,
    Normal,
    Expressive,
}

impl InputPreset {
    const ALL: [Self; 3] = [Self::Soft, Self::Normal, Self::Expressive];

    fn label(self) -> &'static str {
        match self {
            Self::Soft => "Soft",
            Self::Normal => "Normal",
            Self::Expressive => "Expressive",
        }
    }

    fn tag(self) -> NSInteger {
        match self {
            Self::Soft => 0,
            Self::Normal => 1,
            Self::Expressive => 2,
        }
    }

    fn from_tag(tag: i32) -> Option<Self> {
        match tag {
            0 => Some(Self::Soft),
            1 => Some(Self::Normal),
            2 => Some(Self::Expressive),
            _ => None,
        }
    }
}

unsafe fn install_settings_menu(
    app: Id,
    model: &Live2dModel,
    model_path: &str,
    config: &AppConfig,
    camera_status: &str,
    renderer_diagnostics: &RendererDiagnostics,
    state: RuntimeControlState,
) -> Result<SettingsMenu, String> {
    let controller_class = settings_menu_controller_class()?;
    let controller = msg_id(controller_class, "new");
    if controller.is_null() {
        return Err("SettingsMenuController allocation returned nil".to_string());
    }

    let main_menu = ns_menu("")?;
    let app_menu_item = msg_id(msg_id(class("NSMenuItem")?, "alloc"), "init");
    msg_void_id(main_menu, "addItem:", app_menu_item);

    let app_menu = ns_menu("vtube-studio-rs")?;
    msg_void_id(app_menu_item, "setSubmenu:", app_menu);

    add_disabled_menu_item(app_menu, "vtube-studio-rs")?;
    add_separator_menu_item(app_menu)?;

    let model_title = format!("Model: {}", model_title(model_path));
    add_disabled_menu_item(app_menu, &model_title)?;
    add_disabled_menu_item(app_menu, "Model Selection (relaunches app)")?;
    let model_entries = discover_model_menu_entries(model_path);
    let mut model_items = Vec::new();
    if model_entries.is_empty() {
        add_disabled_menu_item(app_menu, "No local models found under public/")?;
    } else {
        for (index, entry) in model_entries.iter().enumerate() {
            let item = add_tagged_action_menu_item(
                app_menu,
                &entry.title,
                "selectModel:",
                "",
                controller,
                index as NSInteger,
            )?;
            set_menu_item_checked(item, entry.current);
            model_items.push(item);
        }
    }
    add_separator_menu_item(app_menu)?;

    add_disabled_menu_item(app_menu, "Expression")?;
    let mut expression_items = Vec::new();
    let none_item = add_tagged_action_menu_item(
        app_menu,
        "None",
        "selectExpression:",
        "",
        controller,
        EXPRESSION_INDEX_NONE as NSInteger,
    )?;
    expression_items.push(none_item);
    if model.expressions.is_empty() {
        add_disabled_menu_item(app_menu, "No expressions in model")?;
    } else {
        for (index, expression) in model.expressions.iter().enumerate() {
            let item = add_tagged_action_menu_item(
                app_menu,
                &expression.name,
                "selectExpression:",
                "",
                controller,
                index as NSInteger,
            )?;
            expression_items.push(item);
        }
    }
    add_separator_menu_item(app_menu)?;

    let diagnostics_item = add_action_menu_item(
        app_menu,
        "Show Diagnostics",
        "toggleDiagnostics:",
        "d",
        controller,
    )?;
    let mouse_item = add_action_menu_item(
        app_menu,
        "Mouse Tracking",
        "toggleMouseTracking:",
        "m",
        controller,
    )?;
    let microphone_item = add_action_menu_item(
        app_menu,
        "Microphone Mouth Input",
        "toggleMicrophoneInput:",
        "u",
        controller,
    )?;
    let camera_title = format!("Camera Tracking: {camera_status}");
    let camera_item = add_action_menu_item(
        app_menu,
        &camera_title,
        "toggleCameraTracking:",
        "c",
        controller,
    )?;
    add_separator_menu_item(app_menu)?;

    add_disabled_menu_item(app_menu, "Mouse Calibration")?;
    let mut mouse_preset_items = Vec::new();
    for preset in InputPreset::ALL {
        let item = add_tagged_action_menu_item(
            app_menu,
            preset.label(),
            "selectMousePreset:",
            "",
            controller,
            preset.tag(),
        )?;
        mouse_preset_items.push(item);
    }
    let mouse_detail = format!(
        "Base: {} | dead {:.2} | eye {:.1}/{:.1} | angle {:.0}/{:.0}/{:.0}",
        mouse_coordinate_space_label(&config.input.mouse),
        config.input.mouse.dead_zone,
        config.input.mouse.eye_x_range,
        config.input.mouse.eye_y_range,
        config.input.mouse.angle_x_degrees,
        config.input.mouse.angle_y_degrees,
        config.input.mouse.angle_z_degrees
    );
    add_disabled_menu_item(app_menu, &mouse_detail)?;
    add_separator_menu_item(app_menu)?;

    add_disabled_menu_item(app_menu, "Mouth Calibration")?;
    let mut mouth_preset_items = Vec::new();
    for preset in InputPreset::ALL {
        let item = add_tagged_action_menu_item(
            app_menu,
            preset.label(),
            "selectMouthPreset:",
            "",
            controller,
            preset.tag(),
        )?;
        mouth_preset_items.push(item);
    }
    let mouth_detail = format!(
        "Base: {} | gate {:.3} | gain {:.1} | curve {:.2} | open {:.2}-{:.2}",
        config.input.microphone.parameter,
        config.input.microphone.noise_gate,
        config.input.microphone.gain,
        config.input.microphone.response_curve,
        config.input.microphone.min_open,
        config.input.microphone.max_open
    );
    add_disabled_menu_item(app_menu, &mouth_detail)?;
    add_separator_menu_item(app_menu)?;

    add_disabled_menu_item(app_menu, "Camera Calibration")?;
    let mut camera_preset_items = Vec::new();
    for preset in InputPreset::ALL {
        let item = add_tagged_action_menu_item(
            app_menu,
            preset.label(),
            "selectCameraPreset:",
            "",
            controller,
            preset.tag(),
        )?;
        camera_preset_items.push(item);
    }
    let camera_detail = format!(
        "Base: {} | dead {:.2} | eye {:.1}/{:.1} | angle {:.0}/{:.0}/{:.0} | mouth gain {:.1}",
        config.input.camera.pose_mode,
        config.input.camera.dead_zone,
        config.input.camera.eye_x_range,
        config.input.camera.eye_y_range,
        config.input.camera.angle_x_degrees,
        config.input.camera.angle_y_degrees,
        config.input.camera.angle_z_degrees,
        config.input.camera.mouth_gain
    );
    add_disabled_menu_item(app_menu, &camera_detail)?;
    add_separator_menu_item(app_menu)?;

    let msaa_title = format!(
        "Renderer MSAA: {}",
        if config.renderer.enable_msaa(config.app.runtime_profile) {
            "on"
        } else {
            "off"
        }
    );
    add_disabled_menu_item(app_menu, &msaa_title)?;
    let masks_title = format!("Renderer masks: {}", renderer_diagnostics.mask_mode);
    add_disabled_menu_item(app_menu, &masks_title)?;
    let texture_title = format!(
        "Texture quality: mipmaps {} / aniso {}",
        if renderer_diagnostics.atlas_mipmaps {
            "on"
        } else {
            "off"
        },
        renderer_diagnostics.atlas_anisotropy
    );
    add_disabled_menu_item(app_menu, &texture_title)?;
    add_separator_menu_item(app_menu)?;

    add_action_menu_item(
        app_menu,
        "Open Active Config...",
        "openActiveConfig:",
        ",",
        controller,
    )?;
    add_disabled_menu_item(app_menu, "Model selection relaunches the app")?;
    add_separator_menu_item(app_menu)?;

    let quit_item = ns_menu_item("Quit vtube-studio-rs", Some("terminate:"), "q")?;
    msg_void_id(app_menu, "addItem:", quit_item);

    msg_void_id(app, "setMainMenu:", main_menu);
    let status_item = install_status_bar_item(app_menu)?;

    let menu = SettingsMenu {
        _controller: controller,
        _status_item: status_item,
        diagnostics_item,
        mouse_item,
        microphone_item,
        camera_item,
        model_items,
        model_entries,
        expression_items,
        mouse_preset_items,
        mouth_preset_items,
        camera_preset_items,
    };
    update_settings_menu_state(&menu, state);
    println!("renderer_event=settings_menu_installed kind=main_menu status_item=VT");
    Ok(menu)
}

unsafe fn handle_settings_menu_commands(
    diagnostics_layer: Id,
    settings_menu: &SettingsMenu,
    diagnostics_visible: &mut bool,
    mouse_enabled: &mut bool,
    microphone_enabled: &mut bool,
    camera_enabled: &mut bool,
    selected_expression_index: &mut Option<usize>,
    mouse_preset: &mut InputPreset,
    mouth_preset: &mut InputPreset,
    camera_preset: &mut InputPreset,
    config: &AppConfig,
    model: &Live2dModel,
    motion_controller: &mut crate::motion::MotionController,
    microphone: &mut Option<MicrophoneInput>,
    camera_input: &mut CameraInput,
) -> Result<(), String> {
    let commands = MENU_COMMANDS.swap(0, Ordering::AcqRel);
    if commands == 0 {
        return Ok(());
    }

    if commands & MENU_TOGGLE_DIAGNOSTICS != 0 {
        *diagnostics_visible = !*diagnostics_visible;
        msg_void_bool(
            diagnostics_layer,
            "setHidden:",
            bool_to_objc(!*diagnostics_visible),
        );
        println!(
            "renderer_event=settings_changed diagnostics_visible={}",
            *diagnostics_visible
        );
    }

    if commands & MENU_TOGGLE_MOUSE != 0 {
        *mouse_enabled = !*mouse_enabled;
        let mouse_config = runtime_mouse_config(&config.input.mouse, *mouse_preset);
        motion_controller.set_mouse_enabled(*mouse_enabled, &mouse_config);
        println!(
            "renderer_event=settings_changed mouse_enabled={}",
            *mouse_enabled
        );
    }

    if commands & MENU_TOGGLE_MICROPHONE != 0 {
        let next_enabled = !*microphone_enabled;
        if next_enabled {
            let microphone_config =
                runtime_microphone_config(&config.input.microphone, *mouth_preset);
            let next_input = MicrophoneInput::from_config(&microphone_config);
            if next_input.is_some() {
                *microphone = next_input;
                motion_controller.set_microphone_enabled(true, &microphone_config);
                *microphone_enabled = true;
            } else {
                *microphone_enabled = false;
            }
        } else {
            *microphone = None;
            motion_controller.set_microphone_enabled(false, &config.input.microphone);
            *microphone_enabled = false;
        }
        println!(
            "renderer_event=settings_changed microphone_enabled={}",
            *microphone_enabled
        );
    }

    if commands & MENU_TOGGLE_CAMERA != 0 {
        let next_enabled = !*camera_enabled;
        let mut camera_config = runtime_camera_config(&config.input.camera, *camera_preset);
        camera_config.enabled = next_enabled;
        *camera_input = CameraInput::from_config(&camera_config);
        *camera_enabled = camera_runtime_active(camera_input.status());
        motion_controller.set_camera_enabled(*camera_enabled, &camera_config);
        println!(
            "renderer_event=settings_changed camera_enabled={} status={}",
            *camera_enabled,
            camera_input.status_label()
        );
    }

    if commands & MENU_SELECT_MOUSE_PRESET != 0 {
        let selected = MENU_SELECTED_MOUSE_PRESET.swap(INPUT_PRESET_UNCHANGED, Ordering::AcqRel);
        if let Some(preset) = InputPreset::from_tag(selected) {
            *mouse_preset = preset;
            if *mouse_enabled {
                let mouse_config = runtime_mouse_config(&config.input.mouse, *mouse_preset);
                motion_controller.set_mouse_enabled(true, &mouse_config);
            }
            println!(
                "renderer_event=settings_changed mouse_preset={}",
                mouse_preset.label()
            );
        }
    }

    if commands & MENU_SELECT_MOUTH_PRESET != 0 {
        let selected = MENU_SELECTED_MOUTH_PRESET.swap(INPUT_PRESET_UNCHANGED, Ordering::AcqRel);
        if let Some(preset) = InputPreset::from_tag(selected) {
            *mouth_preset = preset;
            if *microphone_enabled {
                let microphone_config =
                    runtime_microphone_config(&config.input.microphone, *mouth_preset);
                motion_controller.set_microphone_enabled(true, &microphone_config);
            }
            println!(
                "renderer_event=settings_changed mouth_preset={}",
                mouth_preset.label()
            );
        }
    }

    if commands & MENU_SELECT_CAMERA_PRESET != 0 {
        let selected = MENU_SELECTED_CAMERA_PRESET.swap(INPUT_PRESET_UNCHANGED, Ordering::AcqRel);
        if let Some(preset) = InputPreset::from_tag(selected) {
            *camera_preset = preset;
            let camera_config = runtime_camera_config(&config.input.camera, *camera_preset);
            motion_controller.set_camera_config(&camera_config);
            println!(
                "renderer_event=settings_changed camera_preset={}",
                camera_preset.label()
            );
        }
    }

    if commands & MENU_SELECT_MODEL != 0 {
        let selected = MENU_SELECTED_MODEL_INDEX.swap(MODEL_INDEX_UNCHANGED, Ordering::AcqRel);
        if selected >= 0 {
            let selected = selected as usize;
            if let Some(entry) = settings_menu.model_entries.get(selected) {
                write_selected_model_to_active_config(&entry.path)?;
                update_model_menu_selection(settings_menu, selected);
                schedule_model_relaunch(&entry.path)?;
                println!(
                    "renderer_event=settings_changed selected_model=\"{}\" apply=relaunch config=\"{}\"",
                    entry.path,
                    crate::config::active_config_path()
                );
                terminate_current_app()?;
            }
        }
    }

    if commands & MENU_OPEN_ACTIVE_CONFIG != 0 {
        if let Err(error) = Command::new("open")
            .arg(crate::config::active_config_path())
            .spawn()
        {
            eprintln!(
                "Failed to open active config {}: {error}",
                crate::config::active_config_path()
            );
        }
    }

    if commands & MENU_SELECT_EXPRESSION != 0 {
        let selected =
            MENU_SELECTED_EXPRESSION_INDEX.swap(EXPRESSION_INDEX_UNCHANGED, Ordering::AcqRel);
        match selected {
            EXPRESSION_INDEX_NONE => {
                motion_controller.set_expression(model, None);
                *selected_expression_index = None;
                println!("renderer_event=settings_changed expression=none");
            }
            index if index >= 0 => {
                let expression = model.expressions.get(index as usize);
                if let Some(expression) = expression {
                    if motion_controller.set_expression(model, Some(&expression.name)) {
                        *selected_expression_index = Some(index as usize);
                        println!(
                            "renderer_event=settings_changed expression={}",
                            expression.name
                        );
                    }
                }
            }
            _ => {}
        }
    }

    update_settings_menu_state(
        settings_menu,
        RuntimeControlState {
            diagnostics_visible: *diagnostics_visible,
            mouse_enabled: *mouse_enabled,
            microphone_enabled: *microphone_enabled,
            camera_enabled: *camera_enabled,
            selected_expression_index: *selected_expression_index,
            mouse_preset: *mouse_preset,
            mouth_preset: *mouth_preset,
            camera_preset: *camera_preset,
        },
    );
    Ok(())
}

unsafe fn update_settings_menu_state(menu: &SettingsMenu, state: RuntimeControlState) {
    set_menu_item_checked(menu.diagnostics_item, state.diagnostics_visible);
    set_menu_item_checked(menu.mouse_item, state.mouse_enabled);
    set_menu_item_checked(menu.microphone_item, state.microphone_enabled);
    set_menu_item_checked(menu.camera_item, state.camera_enabled);
    for (item_index, item) in menu.expression_items.iter().enumerate() {
        let checked = match state.selected_expression_index {
            None => item_index == 0,
            Some(expression_index) => item_index == expression_index + 1,
        };
        set_menu_item_checked(*item, checked);
    }
    for (index, item) in menu.mouse_preset_items.iter().enumerate() {
        set_menu_item_checked(*item, InputPreset::ALL[index] == state.mouse_preset);
    }
    for (index, item) in menu.mouth_preset_items.iter().enumerate() {
        set_menu_item_checked(*item, InputPreset::ALL[index] == state.mouth_preset);
    }
    for (index, item) in menu.camera_preset_items.iter().enumerate() {
        set_menu_item_checked(*item, InputPreset::ALL[index] == state.camera_preset);
    }
}

unsafe fn update_model_menu_selection(menu: &SettingsMenu, selected_index: usize) {
    for (index, item) in menu.model_items.iter().enumerate() {
        set_menu_item_checked(*item, index == selected_index);
    }
}

unsafe fn update_camera_menu_status(
    menu: &SettingsMenu,
    title: &str,
    last_title: &mut String,
) -> Result<(), String> {
    if last_title == title {
        return Ok(());
    }

    msg_void_id(menu.camera_item, "setTitle:", ns_string(title)?);
    *last_title = title.to_string();
    Ok(())
}

unsafe fn install_status_bar_item(menu: Id) -> Result<Id, String> {
    let status_bar = msg_id(class("NSStatusBar")?, "systemStatusBar");
    if status_bar.is_null() {
        return Err("NSStatusBar systemStatusBar returned nil".to_string());
    }

    let status_item = msg_id_double(
        status_bar,
        "statusItemWithLength:",
        NS_VARIABLE_STATUS_ITEM_LENGTH,
    );
    if status_item.is_null() {
        return Err("NSStatusBar statusItemWithLength: returned nil".to_string());
    }

    let button = msg_id(status_item, "button");
    if !button.is_null() {
        msg_void_id(button, "setTitle:", ns_string("VT")?);
        msg_void_id(
            button,
            "setToolTip:",
            ns_string("vtube-studio-rs settings")?,
        );
    }
    msg_void_id(status_item, "setMenu:", menu);
    Ok(status_item)
}

unsafe fn ns_menu(title: &str) -> Result<Id, String> {
    let menu = msg_id(class("NSMenu")?, "alloc");
    Ok(msg_id_id(menu, "initWithTitle:", ns_string(title)?))
}

unsafe fn add_disabled_menu_item(menu: Id, title: &str) -> Result<Id, String> {
    let item = ns_menu_item(title, None, "")?;
    msg_void_bool(item, "setEnabled:", NO);
    msg_void_id(menu, "addItem:", item);
    Ok(item)
}

unsafe fn add_action_menu_item(
    menu: Id,
    title: &str,
    action: &str,
    key_equivalent: &str,
    target: Id,
) -> Result<Id, String> {
    let item = ns_menu_item(title, Some(action), key_equivalent)?;
    msg_void_id(item, "setTarget:", target);
    msg_void_id(menu, "addItem:", item);
    Ok(item)
}

unsafe fn add_tagged_action_menu_item(
    menu: Id,
    title: &str,
    action: &str,
    key_equivalent: &str,
    target: Id,
    tag: NSInteger,
) -> Result<Id, String> {
    let item = add_action_menu_item(menu, title, action, key_equivalent, target)?;
    msg_void_int(item, "setTag:", tag);
    Ok(item)
}

unsafe fn add_separator_menu_item(menu: Id) -> Result<(), String> {
    let item = msg_id(class("NSMenuItem")?, "separatorItem");
    msg_void_id(menu, "addItem:", item);
    Ok(())
}

unsafe fn ns_menu_item(
    title: &str,
    action: Option<&str>,
    key_equivalent: &str,
) -> Result<Id, String> {
    let item = msg_id(class("NSMenuItem")?, "alloc");
    let action = action
        .map(|selector_name| selector(selector_name))
        .unwrap_or(ptr::null_mut());
    Ok(msg_id_id_sel_id(
        item,
        "initWithTitle:action:keyEquivalent:",
        ns_string(title)?,
        action,
        ns_string(key_equivalent)?,
    ))
}

unsafe fn set_menu_item_checked(item: Id, checked: bool) {
    msg_void_int(
        item,
        "setState:",
        if checked {
            NS_CONTROL_STATE_VALUE_ON
        } else {
            NS_CONTROL_STATE_VALUE_OFF
        },
    );
}

fn model_title(model_path: &str) -> &str {
    model_path
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(model_path)
}

fn discover_model_menu_entries(current_model_path: &str) -> Vec<ModelMenuEntry> {
    let mut paths = Vec::new();
    if let Err(error) = collect_model3_paths(Path::new("public"), &mut paths) {
        eprintln!("Failed to scan local models for settings menu: {error}");
        return Vec::new();
    }

    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            let path = relative_display_path(&path);
            ModelMenuEntry {
                title: model_menu_title(&path),
                current: model_paths_match(&path, current_model_path),
                path,
            }
        })
        .collect()
}

fn collect_model3_paths(root: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }

    if root.is_file() {
        if is_model3_path(root) {
            paths.push(root.to_path_buf());
        }
        return Ok(());
    }

    for entry in std::fs::read_dir(root)
        .map_err(|error| format!("Failed to read {}: {error}", root.display()))?
    {
        let entry = entry
            .map_err(|error| format!("Failed to read entry in {}: {error}", root.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if file_type.is_dir() {
            collect_model3_paths(&path, paths)?;
        } else if file_type.is_file() && is_model3_path(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

fn is_model3_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".model3.json"))
}

fn relative_display_path(path: &Path) -> String {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or(path);
    relative.to_string_lossy().replace('\\', "/")
}

fn model_menu_title(path: &str) -> String {
    let name = model_title(path);
    let parent = Path::new(path)
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{name} ({parent})")
    }
}

fn model_paths_match(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }

    let left = absolute_path_for_compare(left);
    let right = absolute_path_for_compare(right);
    left == right
}

fn absolute_path_for_compare(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn write_selected_model_to_active_config(model_path: &str) -> Result<(), String> {
    let config_path = Path::new(crate::config::active_config_path());
    let content = if config_path.is_file() {
        std::fs::read_to_string(config_path)
            .map_err(|error| format!("Failed to read {}: {error}", config_path.display()))?
    } else {
        String::new()
    };
    let updated =
        set_toml_section_value(&content, "model", "path", &toml_string_literal(model_path));
    std::fs::write(config_path, updated)
        .map_err(|error| format!("Failed to write {}: {error}", config_path.display()))
}

fn schedule_model_relaunch(model_path: &str) -> Result<(), String> {
    let command_args = relaunch_command_args(model_path)?;
    if command_args.is_empty() {
        return Err("Failed to build relaunch command".to_string());
    }

    let script =
        r#"pid="$1"; shift; while kill -0 "$pid" 2>/dev/null; do sleep 0.1; done; exec "$@""#;
    Command::new("sh")
        .arg("-c")
        .arg(script)
        .arg("vtube-studio-rs-relaunch")
        .arg(std::process::id().to_string())
        .args(command_args)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Failed to schedule model relaunch: {error}"))
}

fn relaunch_command_args(model_path: &str) -> Result<Vec<String>, String> {
    let model_path = absolute_path_for_compare(model_path);
    let config_path = absolute_path_for_compare(crate::config::active_config_path());
    let mut args = Vec::new();

    if let Some(app_bundle) = current_app_bundle_path() {
        args.push("/usr/bin/open".to_string());
        args.push("-n".to_string());
        push_open_env_arg(&mut args, "CUBISM_CORE_INCLUDE_DIR");
        push_open_env_arg(&mut args, "CUBISM_CORE_LIB_DIR");
        args.push(app_bundle.to_string_lossy().to_string());
        args.push("--args".to_string());
    } else {
        let executable = std::env::current_exe()
            .map_err(|error| format!("Failed to resolve current executable: {error}"))?;
        args.push(executable.to_string_lossy().to_string());
    }

    args.push("--config".to_string());
    args.push(config_path.to_string_lossy().to_string());
    args.push(model_path.to_string_lossy().to_string());
    Ok(args)
}

fn push_open_env_arg(args: &mut Vec<String>, name: &str) {
    if let Ok(value) = std::env::var(name) {
        args.push("--env".to_string());
        args.push(format!("{name}={value}"));
    }
}

fn current_app_bundle_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    executable
        .ancestors()
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("app"))
        .map(Path::to_path_buf)
}

unsafe fn terminate_current_app() -> Result<(), String> {
    let app = msg_id(class("NSApplication")?, "sharedApplication");
    msg_void_id(app, "terminate:", NIL);
    Ok(())
}

fn set_toml_section_value(content: &str, section: &str, key: &str, value: &str) -> String {
    let section_header = format!("[{section}]");
    let mut output = String::new();
    let mut found_section = false;
    let mut in_target_section = false;
    let mut found_key = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if in_target_section
            && trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && trimmed != section_header
        {
            if !found_key {
                output.push_str(key);
                output.push_str(" = ");
                output.push_str(value);
                output.push('\n');
                found_key = true;
            }
            in_target_section = false;
        }

        if trimmed == section_header {
            found_section = true;
            in_target_section = true;
        }

        if in_target_section {
            let trimmed_start = line.trim_start();
            if trimmed_start.starts_with(key)
                && trimmed_start[key.len()..].trim_start().starts_with('=')
            {
                let indent_len = line.len() - trimmed_start.len();
                output.push_str(&line[..indent_len]);
                output.push_str(key);
                output.push_str(" = ");
                output.push_str(value);
                output.push('\n');
                found_key = true;
                continue;
            }
        }

        output.push_str(line);
        output.push('\n');
    }

    if found_section && in_target_section && !found_key {
        output.push_str(key);
        output.push_str(" = ");
        output.push_str(value);
        output.push('\n');
    } else if !found_section {
        if !output.is_empty() && !output.ends_with("\n\n") {
            output.push('\n');
        }
        output.push_str(&section_header);
        output.push('\n');
        output.push_str(key);
        output.push_str(" = ");
        output.push_str(value);
        output.push('\n');
    }

    output
}

fn toml_string_literal(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn mouse_coordinate_space_label(config: &MouseConfig) -> &'static str {
    let coordinate_space = config.coordinate_space.trim();
    if coordinate_space.eq_ignore_ascii_case("window") {
        "window"
    } else {
        "screen"
    }
}

fn selected_expression_index(model: &Live2dModel, requested: Option<&str>) -> Option<usize> {
    let requested = requested?;
    model
        .expressions
        .iter()
        .position(|expression| expression.name == requested)
        .or_else(|| {
            requested
                .parse::<usize>()
                .ok()
                .filter(|index| *index < model.expressions.len())
        })
}

fn runtime_mouse_config(base: &MouseConfig, preset: InputPreset) -> MouseConfig {
    let mut config = base.clone();
    config.enabled = true;
    match preset {
        InputPreset::Soft => {
            config.smoothing = (config.smoothing * 0.75).max(1.0);
            config.eye_x_range *= 0.65;
            config.eye_y_range *= 0.65;
            config.angle_x_degrees *= 0.65;
            config.angle_y_degrees *= 0.65;
            config.angle_z_degrees *= 0.65;
        }
        InputPreset::Normal => {}
        InputPreset::Expressive => {
            config.smoothing = (config.smoothing * 1.25).max(1.0);
            config.eye_x_range *= 1.35;
            config.eye_y_range *= 1.35;
            config.angle_x_degrees *= 1.35;
            config.angle_y_degrees *= 1.35;
            config.angle_z_degrees *= 1.35;
        }
    }
    config
}

fn runtime_microphone_config(base: &MicrophoneConfig, preset: InputPreset) -> MicrophoneConfig {
    let mut config = base.clone();
    config.enabled = true;
    match preset {
        InputPreset::Soft => {
            config.gain *= 0.65;
            config.response_curve *= 1.25;
            config.attack *= 0.8;
            config.release *= 0.8;
            config.max_open *= 0.75;
        }
        InputPreset::Normal => {}
        InputPreset::Expressive => {
            config.gain *= 1.45;
            config.response_curve *= 0.75;
            config.attack *= 1.25;
            config.release *= 1.15;
            config.max_open = (config.max_open * 1.15).min(1.0);
        }
    }
    config
}

fn runtime_camera_config(base: &CameraConfig, preset: InputPreset) -> CameraConfig {
    let mut config = base.clone();
    config.enabled = true;
    match preset {
        InputPreset::Soft => {
            config.smoothing = (config.smoothing * 0.8).max(1.0);
            config.eye_x_range *= 0.7;
            config.eye_y_range *= 0.7;
            config.angle_x_degrees *= 0.7;
            config.angle_y_degrees *= 0.7;
            config.angle_z_degrees *= 0.7;
            config.mouth_gain *= 0.75;
            config.mouth_max_open *= 0.85;
        }
        InputPreset::Normal => {}
        InputPreset::Expressive => {
            config.smoothing = (config.smoothing * 1.2).max(1.0);
            config.eye_x_range *= 1.25;
            config.eye_y_range *= 1.25;
            config.angle_x_degrees *= 1.25;
            config.angle_y_degrees *= 1.25;
            config.angle_z_degrees *= 1.25;
            config.mouth_gain *= 1.35;
            config.mouth_max_open = (config.mouth_max_open * 1.1).min(1.0);
        }
    }
    config
}

fn camera_runtime_active(status: CameraStatus) -> bool {
    matches!(status, CameraStatus::Running | CameraStatus::NoFace)
}

unsafe fn settings_menu_controller_class() -> Result<Class, String> {
    let class_name =
        CString::new("VTubeStudioRsSettingsMenuController").map_err(|error| error.to_string())?;
    let existing = objc_getClass(class_name.as_ptr());
    if !existing.is_null() {
        return Ok(existing);
    }

    let superclass = class("NSObject")?;
    let cls = objc_allocateClassPair(superclass, class_name.as_ptr(), 0);
    if cls.is_null() {
        let existing = objc_getClass(class_name.as_ptr());
        if !existing.is_null() {
            return Ok(existing);
        }
        return Err("Failed to allocate VTubeStudioRsSettingsMenuController".to_string());
    }

    add_settings_menu_method(cls, "toggleDiagnostics:", settings_toggle_diagnostics)?;
    add_settings_menu_method(cls, "toggleMouseTracking:", settings_toggle_mouse)?;
    add_settings_menu_method(cls, "toggleMicrophoneInput:", settings_toggle_microphone)?;
    add_settings_menu_method(cls, "toggleCameraTracking:", settings_toggle_camera)?;
    add_settings_menu_method(cls, "openActiveConfig:", settings_open_active_config)?;
    add_settings_menu_method(cls, "selectExpression:", settings_select_expression)?;
    add_settings_menu_method(cls, "selectMousePreset:", settings_select_mouse_preset)?;
    add_settings_menu_method(cls, "selectMouthPreset:", settings_select_mouth_preset)?;
    add_settings_menu_method(cls, "selectCameraPreset:", settings_select_camera_preset)?;
    add_settings_menu_method(cls, "selectModel:", settings_select_model)?;
    objc_registerClassPair(cls);
    Ok(cls)
}

unsafe fn add_settings_menu_method(
    cls: Class,
    name: &str,
    implementation: extern "C" fn(Id, Sel, Id),
) -> Result<(), String> {
    let types = CString::new("v@:@").map_err(|error| error.to_string())?;
    let added = class_addMethod(
        cls,
        selector(name),
        implementation as *const c_void,
        types.as_ptr(),
    );
    if added == YES {
        Ok(())
    } else {
        Err(format!("Failed to add settings menu action {name}"))
    }
}

extern "C" fn settings_toggle_diagnostics(_this: Id, _selector: Sel, _sender: Id) {
    MENU_COMMANDS.fetch_or(MENU_TOGGLE_DIAGNOSTICS, Ordering::AcqRel);
}

extern "C" fn settings_toggle_mouse(_this: Id, _selector: Sel, _sender: Id) {
    MENU_COMMANDS.fetch_or(MENU_TOGGLE_MOUSE, Ordering::AcqRel);
}

extern "C" fn settings_toggle_microphone(_this: Id, _selector: Sel, _sender: Id) {
    MENU_COMMANDS.fetch_or(MENU_TOGGLE_MICROPHONE, Ordering::AcqRel);
}

extern "C" fn settings_toggle_camera(_this: Id, _selector: Sel, _sender: Id) {
    MENU_COMMANDS.fetch_or(MENU_TOGGLE_CAMERA, Ordering::AcqRel);
}

extern "C" fn settings_open_active_config(_this: Id, _selector: Sel, _sender: Id) {
    MENU_COMMANDS.fetch_or(MENU_OPEN_ACTIVE_CONFIG, Ordering::AcqRel);
}

extern "C" fn settings_select_expression(_this: Id, _selector: Sel, sender: Id) {
    unsafe {
        let tag = msg_int(sender, "tag") as i32;
        MENU_SELECTED_EXPRESSION_INDEX.store(tag, Ordering::Release);
        MENU_COMMANDS.fetch_or(MENU_SELECT_EXPRESSION, Ordering::AcqRel);
    }
}

extern "C" fn settings_select_mouse_preset(_this: Id, _selector: Sel, sender: Id) {
    unsafe {
        let tag = msg_int(sender, "tag") as i32;
        MENU_SELECTED_MOUSE_PRESET.store(tag, Ordering::Release);
        MENU_COMMANDS.fetch_or(MENU_SELECT_MOUSE_PRESET, Ordering::AcqRel);
    }
}

extern "C" fn settings_select_mouth_preset(_this: Id, _selector: Sel, sender: Id) {
    unsafe {
        let tag = msg_int(sender, "tag") as i32;
        MENU_SELECTED_MOUTH_PRESET.store(tag, Ordering::Release);
        MENU_COMMANDS.fetch_or(MENU_SELECT_MOUTH_PRESET, Ordering::AcqRel);
    }
}

extern "C" fn settings_select_camera_preset(_this: Id, _selector: Sel, sender: Id) {
    unsafe {
        let tag = msg_int(sender, "tag") as i32;
        MENU_SELECTED_CAMERA_PRESET.store(tag, Ordering::Release);
        MENU_COMMANDS.fetch_or(MENU_SELECT_CAMERA_PRESET, Ordering::AcqRel);
    }
}

extern "C" fn settings_select_model(_this: Id, _selector: Sel, sender: Id) {
    unsafe {
        let tag = msg_int(sender, "tag") as i32;
        MENU_SELECTED_MODEL_INDEX.store(tag, Ordering::Release);
        MENU_COMMANDS.fetch_or(MENU_SELECT_MODEL, Ordering::AcqRel);
    }
}

struct EventPump {
    modes: [Id; 3],
    nonblocking_date: Id,
}

impl EventPump {
    const RUN_LOOP_MODE_NAMES: [&'static str; 3] = [
        "NSDefaultRunLoopMode",
        "NSEventTrackingRunLoopMode",
        "NSModalPanelRunLoopMode",
    ];

    unsafe fn new() -> Result<Self, String> {
        let nonblocking_date = msg_id(class("NSDate")?, "distantPast");
        if nonblocking_date.is_null() {
            return Err("NSDate distantPast returned nil".to_string());
        }

        Ok(Self {
            modes: [
                ns_string(Self::RUN_LOOP_MODE_NAMES[0])?,
                ns_string(Self::RUN_LOOP_MODE_NAMES[1])?,
                ns_string(Self::RUN_LOOP_MODE_NAMES[2])?,
            ],
            nonblocking_date,
        })
    }

    unsafe fn drain_pending_events(&self, app: Id) {
        for mode in self.modes {
            loop {
                let event = msg_id_mask_date_mode_bool(
                    app,
                    "nextEventMatchingMask:untilDate:inMode:dequeue:",
                    NS_EVENT_MASK_ANY,
                    self.nonblocking_date,
                    mode,
                    YES,
                );

                if event.is_null() {
                    break;
                }

                msg_void_id(app, "sendEvent:", event);
                msg_void(app, "updateWindows");
            }
        }
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
    camera_summary: String,
    total_frames: u64,
    slow_frames: u64,
    frames_since_report: u64,
    intervals_since_report: u64,
    interval_sum_since_report: Duration,
    worst_interval_since_report: Duration,
    last_frame_at: Option<Instant>,
    last_report: Instant,
}

struct CameraDebugSummary {
    overlay_text: String,
    menu_text: String,
}

fn camera_debug_summary(
    status: CameraStatus,
    sample: Option<crate::motion::CameraMotionSample>,
    diagnostic: Option<&str>,
) -> CameraDebugSummary {
    let status_label = status.label();
    let Some(sample) = sample else {
        let detail = camera_status_detail(status, diagnostic);
        let overlay_text = match detail.as_deref() {
            Some(detail) => {
                format!("Camera: {status_label} | sample none\nCamera detail: {detail}")
            }
            None => format!("Camera: {status_label} | sample none"),
        };
        return CameraDebugSummary {
            overlay_text,
            menu_text: camera_status_menu_text(status),
        };
    };

    let gaze = sample
        .gaze
        .map(|gaze| format!("{:+.2}/{:+.2}", gaze[0], gaze[1]))
        .unwrap_or_else(|| "none".to_string());
    let mouth = sample
        .mouth_open
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "none".to_string());
    let eye = sample
        .eye_open
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "none".to_string());
    let face_angle = sample
        .face_angle
        .map(|angle| format!("{:+.2}/{:+.2}", angle[0], angle[1]))
        .unwrap_or_else(|| "none".to_string());

    CameraDebugSummary {
        overlay_text: format!(
            "Camera: {status_label} | face {:+.2}/{:+.2} | angle {face_angle} | roll {:+.2}\nCamera sample: gaze {gaze} | mouth {mouth} | eye {eye}",
            sample.face_offset[0], sample.face_offset[1], sample.face_roll,
        ),
        menu_text: format!("Camera Tracking: {status_label} | mouth {mouth} | gaze {gaze}"),
    }
}

fn camera_status_detail(status: CameraStatus, diagnostic: Option<&str>) -> Option<String> {
    match status {
        CameraStatus::Disabled | CameraStatus::Running => None,
        CameraStatus::NoFace => Some(
            "No face detected; improve lighting, center your face, or check camera framing."
                .to_string(),
        ),
        CameraStatus::WaitingForPermission
        | CameraStatus::PermissionDenied
        | CameraStatus::NoCamera
        | CameraStatus::BackendPending
        | CameraStatus::Failed => diagnostic
            .and_then(first_non_empty_line)
            .map(str::to_string)
            .or_else(|| Some(camera_status_fallback_detail(status).to_string())),
    }
}

fn first_non_empty_line(value: &str) -> Option<&str> {
    value.lines().map(str::trim).find(|line| !line.is_empty())
}

fn camera_status_fallback_detail(status: CameraStatus) -> &'static str {
    match status {
        CameraStatus::Disabled => "Camera tracking is disabled.",
        CameraStatus::WaitingForPermission => "Approve the macOS camera permission prompt.",
        CameraStatus::PermissionDenied => {
            "Enable Camera permission for vtube-studio-rs Dev in System Settings."
        }
        CameraStatus::NoCamera => "No matching camera was found.",
        CameraStatus::BackendPending => "Rebuild with the camera-tracking feature.",
        CameraStatus::Running => "Camera tracking is running.",
        CameraStatus::NoFace => "No face detected.",
        CameraStatus::Failed => "Camera setup failed; check the terminal log.",
    }
}

fn camera_status_menu_text(status: CameraStatus) -> String {
    let status_label = status.label();
    let hint = match status {
        CameraStatus::Disabled | CameraStatus::Running => None,
        CameraStatus::WaitingForPermission => Some("approve prompt"),
        CameraStatus::PermissionDenied => Some("open Camera privacy"),
        CameraStatus::NoCamera => Some("check device"),
        CameraStatus::BackendPending => Some("camera feature missing"),
        CameraStatus::NoFace => Some("adjust framing"),
        CameraStatus::Failed => Some("see diagnostics"),
    };

    match hint {
        Some(hint) => format!("Camera Tracking: {status_label} | {hint}"),
        None => format!("Camera Tracking: {status_label}"),
    }
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

    unsafe fn poll(
        &mut self,
        app: Id,
        window: Id,
        app_config: &AppRuntimeConfig,
        started_at: Instant,
    ) {
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

        if !window_visible || !window_occlusion_visible(occlusion_state) {
            apply_avatar_window_space_policy(window, app_config);
            msg_void(window, "orderFrontRegardless");
            println!(
                "renderer_event=window_reasserted visible={} occlusion_state={} uptime_s={uptime:.1}",
                window_visible, occlusion_state
            );
        }
    }
}

fn window_occlusion_visible(occlusion_state: NSUInteger) -> bool {
    occlusion_state & NS_WINDOW_OCCLUSION_STATE_VISIBLE != 0
}

#[derive(Debug, Clone)]
struct RendererDiagnostics {
    mask_mode: String,
    debug_texture_mode: String,
    atlas_mipmaps: bool,
    atlas_anisotropy: u64,
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
            atlas_anisotropy: config.atlas_anisotropy.clamp(1, 16),
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
            "Renderer: mask {} | offscreen {} | ext blend {} | debug {} | mipmaps {} | aniso {} | filters h/o/hi {}/{}/{}",
            self.mask_mode,
            self.offscreen_count,
            self.extended_blend_count,
            self.debug_texture_mode,
            if self.atlas_mipmaps { "on" } else { "off" },
            self.atlas_anisotropy,
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
        camera_summary: String,
    ) -> Self {
        let now = Instant::now();
        Self {
            target_frame_duration,
            model_summary: diagnostics_model_summary(&model_summary),
            cubism_summary: diagnostics_cubism_summary(&cubism_summary),
            renderer_summary: diagnostics_renderer_summary(&renderer_diagnostics.summary()),
            camera_summary,
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

    fn set_camera_summary(&mut self, camera_summary: String) {
        self.camera_summary = camera_summary;
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
            "Model: {}\n{}\n{}\n{}\nFPS: {:>5.1} / {:.0}\nFrame delta: avg {:>5.1} ms, max {:>5.1} ms\nBudget: {:>5.1} ms\nSlow frames: {}\nFrames: {}\nUptime: {:>6.1}s\nApp Nap guard: active",
            self.model_summary,
            self.cubism_summary,
            self.renderer_summary,
            self.camera_summary,
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

fn diagnostics_model_summary(summary: &str) -> String {
    summary.replace(" | groups: ", "\nGroups: ")
}

fn diagnostics_cubism_summary(summary: &str) -> String {
    summary
        .replace(" | drawables ", "\nDrawables ")
        .replace(" | canvas ", "\nCanvas ")
}

fn diagnostics_renderer_summary(summary: &str) -> String {
    summary
        .replace(" | debug ", "\nRenderer debug ")
        .replace(" | filters ", "\nRenderer filters ")
}

fn bool_to_objc(value: bool) -> Bool {
    if value { YES } else { NO }
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

unsafe fn msg_int(receiver: Id, selector_name: &str) -> NSInteger {
    let function: extern "C" fn(Id, Sel) -> NSInteger =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name))
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

unsafe fn msg_id_id(receiver: Id, selector_name: &str, value: Id) -> Id {
    let function: extern "C" fn(Id, Sel, Id) -> Id = std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), value)
}

unsafe fn msg_id_double(receiver: Id, selector_name: &str, value: CGFloat) -> Id {
    let function: extern "C" fn(Id, Sel, CGFloat) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), value)
}

unsafe fn msg_id_id_sel_id(
    receiver: Id,
    selector_name: &str,
    title: Id,
    action: Sel,
    key: Id,
) -> Id {
    let function: extern "C" fn(Id, Sel, Id, Sel, Id) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), title, action, key)
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
    use super::{
        EventPump, InputPreset, NS_NONACTIVATING_PANEL_MASK,
        NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_APPLICATIONS,
        NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES,
        NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY,
        NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE, NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY,
        NS_WINDOW_OCCLUSION_STATE_VISIBLE, NSPoint, NSRect, NSSize, avatar_frame_for_bounds,
        avatar_window_collection_behavior, avatar_window_level_key, avatar_window_level_name,
        avatar_window_size, avatar_window_style_mask, camera_debug_summary, camera_runtime_active,
        is_model3_path, model_menu_title, model_paths_match, model_title,
        mouse_coordinate_space_label, normalized_point_in_rect, relaunch_command_args,
        runtime_camera_config, runtime_microphone_config, runtime_mouse_config,
        selected_expression_index, set_toml_section_value, toml_string_literal,
        window_occlusion_visible,
    };
    use crate::camera_input::CameraStatus;
    use crate::config::{AppRuntimeConfig, CameraConfig, MicrophoneConfig, MouseConfig};
    use crate::live2d_model::{Live2dModel, ModelExpression};
    use crate::motion::CameraMotionSample;
    use std::collections::HashMap;
    use std::path::PathBuf;

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

    #[test]
    fn avatar_window_policy_is_space_resilient_overlay() {
        let behavior = avatar_window_collection_behavior();
        let configured_size = avatar_window_size(&AppRuntimeConfig {
            window_width: 540.0,
            window_height: 720.0,
            ..AppRuntimeConfig::default()
        });
        let fallback_size = avatar_window_size(&AppRuntimeConfig {
            window_width: 0.0,
            window_height: f64::NAN,
            ..AppRuntimeConfig::default()
        });

        assert!(avatar_window_style_mask() & NS_NONACTIVATING_PANEL_MASK != 0);
        assert_eq!(configured_size.width, 540.0);
        assert_eq!(configured_size.height, 720.0);
        assert_eq!(fallback_size.width, 360.0);
        assert_eq!(fallback_size.height, 480.0);
        assert_eq!(avatar_window_level_name(""), "screen_saver");
        assert_eq!(avatar_window_level_name("screen-saver"), "screen_saver");
        assert_eq!(avatar_window_level_name("max"), "maximum");
        assert_eq!(avatar_window_level_key(""), 13);
        assert_eq!(avatar_window_level_key("overlay"), 15);
        assert_eq!(avatar_window_level_key("screen_saver"), 13);
        assert_eq!(avatar_window_level_key("maximum"), 14);
        assert!(behavior & NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES != 0);
        assert!(behavior & NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_APPLICATIONS != 0);
        assert!(behavior & NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY != 0);
        assert!(behavior & NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE != 0);
        assert!(behavior & NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY != 0);
    }

    #[test]
    fn event_pump_uses_nonblocking_space_transition_modes() {
        assert_eq!(EventPump::RUN_LOOP_MODE_NAMES.len(), 3);
        assert!(EventPump::RUN_LOOP_MODE_NAMES.contains(&"NSDefaultRunLoopMode"));
        assert!(EventPump::RUN_LOOP_MODE_NAMES.contains(&"NSEventTrackingRunLoopMode"));
        assert!(EventPump::RUN_LOOP_MODE_NAMES.contains(&"NSModalPanelRunLoopMode"));
    }

    #[test]
    fn occlusion_state_visible_bit_drives_space_reassertion() {
        assert!(window_occlusion_visible(NS_WINDOW_OCCLUSION_STATE_VISIBLE));
        assert!(window_occlusion_visible(
            8192 | NS_WINDOW_OCCLUSION_STATE_VISIBLE
        ));
        assert!(!window_occlusion_visible(0));
        assert!(!window_occlusion_visible(8192));
    }

    #[test]
    fn model_title_uses_file_name_when_path_has_directories() {
        assert_eq!(
            model_title("public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json"),
            "Rice.model3.json"
        );
        assert_eq!(model_title("0.model3.json"), "0.model3.json");
    }

    #[test]
    fn model_menu_title_includes_parent_folder() {
        assert_eq!(
            model_menu_title("public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json"),
            "Rice.model3.json (Rice)"
        );
        assert_eq!(model_menu_title("0.model3.json"), "0.model3.json");
    }

    #[test]
    fn model_path_helpers_match_model3_manifests() {
        assert!(is_model3_path(std::path::Path::new(
            "public/model/0.model3.json"
        )));
        assert!(!is_model3_path(std::path::Path::new(
            "public/model/model.json"
        )));
        assert!(model_paths_match(
            "public/model/0.model3.json",
            "public/model/0.model3.json"
        ));
    }

    #[test]
    fn set_toml_section_value_updates_model_path_only_in_model_section() {
        let content = "[other]\npath = \"keep\"\n\n[model]\n  path = \"old\"\n[renderer]\n";
        let updated =
            set_toml_section_value(content, "model", "path", "\"public/model/0.model3.json\"");

        assert!(updated.contains("[other]\npath = \"keep\"\n"));
        assert!(updated.contains("[model]\n  path = \"public/model/0.model3.json\"\n"));
        assert!(updated.contains("[renderer]\n"));
    }

    #[test]
    fn set_toml_section_value_appends_missing_model_section() {
        let updated =
            set_toml_section_value("[renderer]\n", "model", "path", "\"avatar.model3.json\"");

        assert!(updated.ends_with("\n[model]\npath = \"avatar.model3.json\"\n"));
    }

    #[test]
    fn toml_string_literal_escapes_model_paths() {
        assert_eq!(
            toml_string_literal(r#"public/model/"avatar"\0.model3.json"#),
            r#""public/model/\"avatar\"\\0.model3.json""#
        );
    }

    #[test]
    fn relaunch_command_passes_selected_model_as_cli_argument() {
        let args = relaunch_command_args("public/model/0.model3.json")
            .expect("relaunch args should build");

        assert!(args.iter().any(|arg| arg == "--config"));
        assert!(
            args.last()
                .is_some_and(|arg| arg.ends_with("public/model/0.model3.json"))
        );
    }

    #[test]
    fn selected_expression_index_matches_name_or_numeric_index() {
        let model = Live2dModel {
            manifest_path: PathBuf::from("model.model3.json"),
            root_dir: PathBuf::from("."),
            version: 3,
            moc: PathBuf::from("model.moc3"),
            textures: Vec::new(),
            physics: None,
            display_info: None,
            motions: HashMap::new(),
            expressions: vec![
                ModelExpression {
                    name: "smile".to_string(),
                    file: PathBuf::from("smile.exp3.json"),
                },
                ModelExpression {
                    name: "angry".to_string(),
                    file: PathBuf::from("angry.exp3.json"),
                },
            ],
            groups: Vec::new(),
        };

        assert_eq!(selected_expression_index(&model, Some("smile")), Some(0));
        assert_eq!(selected_expression_index(&model, Some("1")), Some(1));
        assert_eq!(selected_expression_index(&model, Some("missing")), None);
        assert_eq!(selected_expression_index(&model, Some("2")), None);
        assert_eq!(selected_expression_index(&model, None), None);
    }

    #[test]
    fn input_presets_scale_mouse_and_microphone_runtime_configs() {
        let mouse = MouseConfig {
            enabled: false,
            coordinate_space: "screen".to_string(),
            smoothing: 10.0,
            dead_zone: 0.02,
            invert_x: false,
            invert_y: false,
            eye_x_range: 1.0,
            eye_y_range: 1.0,
            angle_x_degrees: 30.0,
            angle_y_degrees: 20.0,
            angle_z_degrees: -10.0,
        };
        let soft_mouse = runtime_mouse_config(&mouse, InputPreset::Soft);
        let expressive_mouse = runtime_mouse_config(&mouse, InputPreset::Expressive);
        assert!(soft_mouse.enabled);
        assert!(soft_mouse.angle_x_degrees < mouse.angle_x_degrees);
        assert!(expressive_mouse.angle_x_degrees > mouse.angle_x_degrees);
        assert!(expressive_mouse.eye_x_range > soft_mouse.eye_x_range);

        let microphone = MicrophoneConfig {
            enabled: false,
            parameter: "ParamMouthOpenY".to_string(),
            gain: 7.0,
            noise_gate: 0.025,
            response_curve: 0.6,
            smoothing: 18.0,
            attack: 24.0,
            release: 12.0,
            min_open: 0.0,
            max_open: 0.8,
        };
        let soft_mouth = runtime_microphone_config(&microphone, InputPreset::Soft);
        let expressive_mouth = runtime_microphone_config(&microphone, InputPreset::Expressive);
        assert!(soft_mouth.enabled);
        assert!(soft_mouth.gain < microphone.gain);
        assert!(expressive_mouth.gain > microphone.gain);
        assert!(expressive_mouth.max_open > soft_mouth.max_open);

        let camera = CameraConfig {
            enabled: false,
            angle_x_degrees: 30.0,
            angle_y_degrees: 20.0,
            angle_z_degrees: 10.0,
            eye_x_range: 1.0,
            eye_y_range: 1.0,
            mouth_gain: 1.4,
            mouth_max_open: 0.9,
            ..CameraConfig::default()
        };
        let soft_camera = runtime_camera_config(&camera, InputPreset::Soft);
        let expressive_camera = runtime_camera_config(&camera, InputPreset::Expressive);
        assert!(soft_camera.enabled);
        assert!(soft_camera.angle_x_degrees < camera.angle_x_degrees);
        assert!(expressive_camera.angle_x_degrees > camera.angle_x_degrees);
        assert!(expressive_camera.mouth_gain > soft_camera.mouth_gain);
    }

    #[test]
    fn normalized_point_in_rect_uses_window_relative_coordinates() {
        let frame = NSRect {
            origin: NSPoint { x: 100.0, y: 200.0 },
            size: NSSize {
                width: 400.0,
                height: 200.0,
            },
        };

        assert_eq!(
            normalized_point_in_rect(NSPoint { x: 300.0, y: 300.0 }, frame),
            Some([0.0, 0.0])
        );
        assert_eq!(
            normalized_point_in_rect(NSPoint { x: 500.0, y: 400.0 }, frame),
            Some([1.0, 1.0])
        );
        assert_eq!(
            normalized_point_in_rect(NSPoint { x: 0.0, y: 100.0 }, frame),
            Some([-1.0, -1.0])
        );
    }

    #[test]
    fn mouse_coordinate_space_defaults_to_screen() {
        let mut mouse = MouseConfig::default();
        assert_eq!(mouse_coordinate_space_label(&mouse), "screen");
        mouse.coordinate_space = "window".to_string();
        assert_eq!(mouse_coordinate_space_label(&mouse), "window");
        mouse.coordinate_space = "".to_string();
        assert_eq!(mouse_coordinate_space_label(&mouse), "screen");
    }

    #[test]
    fn camera_debug_summary_includes_status_and_sample_values() {
        let summary = camera_debug_summary(
            CameraStatus::Running,
            Some(CameraMotionSample {
                face_offset: [0.25, -0.5],
                face_angle: Some([0.15, -0.2]),
                face_roll: 0.125,
                gaze: Some([0.75, -0.25]),
                mouth_open: Some(0.42),
                eye_open: Some(0.88),
            }),
            None,
        );

        assert!(summary.overlay_text.contains("Camera: running"));
        assert!(summary.overlay_text.contains("face +0.25/-0.50"));
        assert!(summary.overlay_text.contains("mouth 0.42"));
        assert!(summary.menu_text.contains("Camera Tracking: running"));
    }

    #[test]
    fn camera_debug_summary_surfaces_actionable_non_running_statuses() {
        let permission = camera_debug_summary(
            CameraStatus::PermissionDenied,
            None,
            Some("Camera permission is denied or restricted.\nCamera tracking is local-only."),
        );
        assert!(
            permission
                .overlay_text
                .contains("Camera permission is denied or restricted.")
        );
        assert!(
            permission
                .menu_text
                .contains("Camera Tracking: permission denied | open Camera privacy")
        );

        let no_face = camera_debug_summary(CameraStatus::NoFace, None, None);
        assert!(no_face.overlay_text.contains("No face detected"));
        assert!(
            no_face
                .menu_text
                .contains("Camera Tracking: no face | adjust framing")
        );
    }

    #[test]
    fn camera_runtime_active_matches_capture_usable_states() {
        assert!(camera_runtime_active(CameraStatus::Running));
        assert!(camera_runtime_active(CameraStatus::NoFace));
        assert!(!camera_runtime_active(CameraStatus::Disabled));
        assert!(!camera_runtime_active(CameraStatus::PermissionDenied));
        assert!(!camera_runtime_active(CameraStatus::NoCamera));
        assert!(!camera_runtime_active(CameraStatus::Failed));
    }
}
