#![cfg(target_os = "macos")]
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{c_char, c_double, c_long, c_ulong, c_void, CString};
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

pub fn run() -> Result<(), String> {
    unsafe {
        let app = msg_id(class("NSApplication")?, "sharedApplication");
        msg_void_id(app, "setActivationPolicy:", NS_APPLICATION_ACTIVATION_POLICY_ACCESSORY);

        let window = create_avatar_window()?;
        let root_layer = create_root_layer(window)?;
        let avatar_layer = create_avatar_layer()?;
        msg_void_id(root_layer, "addSublayer:", avatar_layer);

        msg_void_id(window, "makeKeyAndOrderFront:", NIL);
        msg_void(window, "orderFrontRegardless");
        msg_void_id(app, "activateIgnoringOtherApps:", YES);

        let run_loop_mode = ns_string("kCFRunLoopDefaultMode")?;
        let mut frame_clock = FrameClock::new(60.0);
        let started_at = Instant::now();

        loop {
            drain_pending_events(app, run_loop_mode);
            draw_avatar_frame(avatar_layer, started_at.elapsed().as_secs_f64())?;
            frame_clock.sleep_until_next_frame();
        }
    }
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
    let color = ns_color(0.04, 0.05, 0.07, 0.72)?;
    let cg_color = msg_id(color, "CGColor");
    msg_void_id(layer, "setBackgroundColor:", cg_color);
    msg_void_double(layer, "setCornerRadius:", 24.0);

    Ok(layer)
}

unsafe fn create_avatar_layer() -> Result<Id, String> {
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

    Ok(layer)
}

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

unsafe fn class(name: &str) -> Result<Class, String> {
    let name = CString::new(name).map_err(|error| error.to_string())?;
    let class = objc_getClass(name.as_ptr());
    if class.is_null() {
        Err(format!("Objective-C class not found: {}", name.to_string_lossy()))
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

unsafe fn msg_void_point(receiver: Id, selector_name: &str, point: NSPoint) {
    msg_void_id(receiver, selector_name, point);
}

unsafe fn msg_id_cstr(receiver: Id, selector_name: &str, value: *const c_char) -> Id {
    let function: extern "C" fn(Id, Sel, *const c_char) -> Id =
        std::mem::transmute(objc_msgSend as *const ());
    function(receiver, selector(selector_name), value)
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
    function(receiver, selector(selector_name), rect, style, backing, defer)
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
