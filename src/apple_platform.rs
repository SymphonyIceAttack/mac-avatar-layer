#![cfg(target_os = "macos")]

use core::ffi::c_void;
use std::ffi::{CStr, CString};
use std::path::Path;

#[cfg(not(feature = "metal-renderer"))]
use objc2::AnyThread;
#[cfg(any(feature = "camera-tracking", feature = "screen-capture-kit"))]
use objc2::rc::Retained;
use objc2::rc::Retained as ObjcRetained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, msg_send};
#[cfg(not(feature = "metal-renderer"))]
use objc2_app_kit::NSImage;
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSControlStateValueOff, NSControlStateValueOn,
    NSEventMask, NSMenu, NSMenuItem, NSPanel, NSStatusBar, NSVariableStatusItemLength, NSView,
    NSWindow, NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask, NSWorkspace,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGWindowLevelForKey, CGWindowLevelKey};
#[cfg(any(feature = "camera-tracking", feature = "screen-capture-kit"))]
use objc2_foundation::NSError;
use objc2_foundation::{NSArray, NSDate, NSRunLoopMode, NSString, NSURL};
#[cfg(not(feature = "metal-renderer"))]
use objc2_quartz_core::kCAGravityResizeAspectFill;
use objc2_quartz_core::{CALayer, CATextLayer, CATransaction};

pub type MenuActionImplementation = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);

#[derive(Clone, Copy)]
pub struct LayerFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy)]
pub struct TextLayerStyle {
    pub frame: LayerFrame,
    pub foreground_color: *mut c_void,
    pub font_size: f64,
    pub contents_scale: f64,
    pub z_position: f64,
    pub wrapped: bool,
}

#[cfg(not(feature = "metal-renderer"))]
#[derive(Clone, Copy)]
pub struct ImageLayerStyle {
    pub frame: LayerFrame,
    pub corner_radius: f64,
    pub masks_to_bounds: bool,
    pub allows_edge_antialiasing: bool,
}

#[derive(Clone, Copy)]
pub struct PanelStyle {
    pub frame: LayerFrame,
    pub style_mask: u64,
    pub level: i64,
    pub collection_behavior: u64,
    pub title: &'static str,
    pub excluded_from_windows_menu: bool,
    pub sharing_read_only: bool,
}

#[cfg(any(feature = "camera-tracking", feature = "screen-capture-kit"))]
pub fn foundation_string(value: &Retained<NSString>) -> String {
    value.to_string()
}

#[cfg(any(feature = "camera-tracking", feature = "screen-capture-kit"))]
pub fn ns_error_description(error: Retained<NSError>) -> String {
    foundation_string(&error.localizedDescription())
}

pub fn settings_menu_controller_class(
    class_name: &'static CStr,
    methods: &[(&str, MenuActionImplementation)],
) -> Result<*mut c_void, String> {
    if let Some(existing) = AnyClass::get(class_name) {
        return Ok(class_ptr(existing));
    }

    let superclass =
        AnyClass::get(c"NSObject").ok_or_else(|| "NSObject class not found".to_string())?;
    let mut builder = ClassBuilder::new(class_name, superclass).ok_or_else(|| {
        if AnyClass::get(class_name).is_some() {
            format!("Settings menu controller class already exists: {class_name:?}")
        } else {
            format!("Failed to allocate settings menu controller class: {class_name:?}")
        }
    })?;

    for (name, implementation) in methods {
        let selector = selector_for_name(name)?;
        unsafe {
            builder.add_method::<AnyObject, _>(selector, *implementation);
        }
    }

    Ok(class_ptr(builder.register()))
}

pub unsafe fn new_object_from_class(class: *mut c_void) -> Result<*mut c_void, String> {
    let class = unsafe { borrowed_class(class) };
    let object: ObjcRetained<AnyObject> = unsafe { msg_send![class, new] };
    Ok(ObjcRetained::into_raw(object).cast())
}

pub unsafe fn configure_view_backed_root_layer(
    content_view: *mut c_void,
    background_color: *mut c_void,
) -> Result<*mut c_void, String> {
    let content_view = unsafe { borrowed_view(content_view) };
    content_view.setWantsLayer(true);
    let layer = content_view
        .layer()
        .ok_or_else(|| "contentView layer returned nil".to_string())?;
    configure_transparent_root_layer(&layer, unsafe { borrowed_cg_color(background_color) });
    Ok(ObjcRetained::into_raw(layer).cast())
}

pub unsafe fn create_transparent_panel(style: PanelStyle) -> Result<*mut c_void, String> {
    let mtm = main_thread_marker()?;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        cg_rect(style.frame),
        NSWindowStyleMask(style.style_mask as usize),
        NSBackingStoreType::Buffered,
        false,
    );
    configure_transparent_panel(
        &panel,
        style.level,
        style.collection_behavior,
        style.title,
        style.excluded_from_windows_menu,
        style.sharing_read_only,
    );
    Ok(ObjcRetained::into_raw(panel).cast())
}

pub unsafe fn set_panel_space_policy(panel: *mut c_void, level: i64, collection_behavior: u64) {
    let window = unsafe { borrowed_window(panel) };
    window.setLevel(level as isize);
    window.setCollectionBehavior(NSWindowCollectionBehavior(collection_behavior as usize));
}

pub unsafe fn window_content_view(window: *mut c_void) -> Result<*mut c_void, String> {
    let window = unsafe { borrowed_window(window) };
    let content_view = window
        .contentView()
        .ok_or_else(|| "window contentView returned nil".to_string())?;
    Ok(ObjcRetained::into_raw(content_view).cast())
}

#[cfg(feature = "metal-renderer")]
pub unsafe fn window_backing_scale_factor(window: *mut c_void) -> f64 {
    let window = unsafe { borrowed_window(window) };
    window.backingScaleFactor()
}

pub unsafe fn application_is_active(app: *mut c_void) -> bool {
    let app = unsafe { borrowed_application(app) };
    app.isActive()
}

pub unsafe fn panel_is_visible(panel: *mut c_void) -> bool {
    let window = unsafe { borrowed_window(panel) };
    window.isVisible()
}

pub unsafe fn panel_occlusion_state(panel: *mut c_void) -> u64 {
    let window = unsafe { borrowed_window(panel) };
    window.occlusionState().0 as u64
}

pub unsafe fn panel_window_number(panel: *mut c_void) -> i64 {
    let panel = unsafe { borrowed_panel(panel) };
    panel.windowNumber() as i64
}

pub unsafe fn order_panel_front_regardless(panel: *mut c_void) {
    let window = unsafe { borrowed_window(panel) };
    window.orderFrontRegardless();
}

pub unsafe fn set_window_origin(window: *mut c_void, x: f64, y: f64) {
    let window = unsafe { borrowed_window(window) };
    window.setFrameOrigin(CGPoint { x, y });
}

pub fn distant_past_date() -> *mut c_void {
    ObjcRetained::into_raw(NSDate::distantPast()).cast()
}

pub unsafe fn drain_pending_application_events(
    app: *mut c_void,
    nonblocking_date: *mut c_void,
    mode: *mut c_void,
    event_mask: u64,
) {
    let app = unsafe { borrowed_application(app) };
    let nonblocking_date = unsafe { borrowed_date(nonblocking_date) };
    let mode = unsafe { borrowed_run_loop_mode(mode) };
    while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
        NSEventMask(event_mask),
        Some(nonblocking_date),
        mode,
        true,
    ) {
        app.sendEvent(&event);
        app.updateWindows();
    }
}

pub fn window_level_for_key(key: i32) -> i64 {
    CGWindowLevelForKey(CGWindowLevelKey(key)) as i64
}

#[cfg(not(feature = "metal-renderer"))]
pub unsafe fn create_image_layer(style: ImageLayerStyle) -> Result<*mut c_void, String> {
    let layer = CALayer::layer();
    layer.setFrame(cg_rect(style.frame));
    layer.setCornerRadius(style.corner_radius);
    layer.setMasksToBounds(style.masks_to_bounds);
    layer.setAllowsEdgeAntialiasing(style.allows_edge_antialiasing);
    layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
    Ok(ObjcRetained::into_raw(layer).cast())
}

#[cfg(not(feature = "metal-renderer"))]
pub unsafe fn set_layer_image_from_file(layer: *mut c_void, path: &Path) -> Result<(), String> {
    let layer = unsafe { borrowed_layer(layer) };
    let path = path
        .to_str()
        .ok_or_else(|| format!("Texture path is not valid UTF-8: {}", path.display()))?;
    let _mtm = main_thread_marker()?;
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path))
        .ok_or_else(|| format!("Failed to load texture image: {path}"))?;
    unsafe {
        layer.setContents(Some(image.as_super()));
    }
    Ok(())
}

#[cfg(not(feature = "cubism-core"))]
pub unsafe fn set_layer_background_color(layer: *mut c_void, background_color: *mut c_void) {
    let layer = unsafe { borrowed_layer(layer) };
    layer.setBackgroundColor(Some(unsafe { borrowed_cg_color(background_color) }));
}

#[cfg(not(feature = "cubism-core"))]
pub unsafe fn set_layer_position(layer: *mut c_void, x: f64, y: f64) {
    let layer = unsafe { borrowed_layer(layer) };
    layer.setPosition(CGPoint { x, y });
}

pub unsafe fn create_text_layer(style: TextLayerStyle, text: &str) -> Result<*mut c_void, String> {
    let layer = CATextLayer::layer();
    layer.setFrame(cg_rect(style.frame));
    layer.setForegroundColor(Some(unsafe { borrowed_cg_color(style.foreground_color) }));
    layer.setFontSize(style.font_size);
    layer.setContentsScale(style.contents_scale);
    layer.setZPosition(style.z_position);
    layer.setWrapped(style.wrapped);
    unsafe {
        set_text_layer_string(&layer, text);
    }
    Ok(ObjcRetained::into_raw(layer).cast())
}

pub unsafe fn set_text_layer_text(layer: *mut c_void, text: &str) {
    let layer = unsafe { borrowed_text_layer(layer) };
    unsafe {
        set_text_layer_string(layer, text);
    }
}

pub unsafe fn set_layer_hidden(layer: *mut c_void, hidden: bool) {
    let layer = unsafe { borrowed_layer(layer) };
    layer.setHidden(hidden);
}

#[cfg(feature = "metal-renderer")]
pub unsafe fn layer_bounds(layer: *mut c_void) -> LayerFrame {
    let layer = unsafe { borrowed_layer(layer) };
    layer_frame(layer.bounds())
}

pub unsafe fn add_sublayer(parent: *mut c_void, child: *mut c_void) {
    let parent = unsafe { borrowed_layer(parent) };
    let child = unsafe { borrowed_layer(child) };
    parent.addSublayer(child);
}

#[cfg(feature = "metal-renderer")]
pub unsafe fn install_metal_layer(parent: *mut c_void, layer: *mut c_void, frame: LayerFrame) {
    let layer = unsafe { borrowed_layer(layer) };
    layer.setFrame(cg_rect(frame));
    layer.setZPosition(1.0);
    layer.setAllowsEdgeAntialiasing(true);
    let parent = unsafe { borrowed_layer(parent) };
    parent.addSublayer(layer);
}

#[cfg(feature = "metal-renderer")]
pub unsafe fn sync_metal_layer_geometry(
    layer: *mut c_void,
    frame: LayerFrame,
    contents_scale: f64,
) {
    let layer = unsafe { borrowed_layer(layer) };
    layer.setFrame(cg_rect(frame));
    layer.setContentsScale(contents_scale);
}

pub fn begin_immediate_layer_update() {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
}

pub fn commit_layer_update() {
    CATransaction::commit();
}

pub fn new_menu(title: &str) -> Result<*mut c_void, String> {
    let mtm = main_thread_marker()?;
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    Ok(ObjcRetained::into_raw(menu).cast())
}

pub unsafe fn new_menu_item(
    title: &str,
    action: Option<&str>,
    key_equivalent: &str,
) -> Result<*mut c_void, String> {
    let mtm = main_thread_marker()?;
    let action = action.map(selector_for_name).transpose()?;
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key_equivalent),
        )
    };
    Ok(ObjcRetained::into_raw(item).cast())
}

pub fn new_separator_menu_item() -> Result<*mut c_void, String> {
    let item = NSMenuItem::separatorItem(main_thread_marker()?);
    Ok(ObjcRetained::into_raw(item).cast())
}

pub unsafe fn add_menu_item(menu: *mut c_void, item: *mut c_void) {
    let menu = unsafe { borrowed_menu(menu) };
    let item = unsafe { borrowed_menu_item(item) };
    menu.addItem(item);
}

pub unsafe fn set_menu_item_submenu(item: *mut c_void, submenu: *mut c_void) {
    let item = unsafe { borrowed_menu_item(item) };
    let submenu = unsafe { borrowed_menu(submenu) };
    item.setSubmenu(Some(submenu));
}

pub unsafe fn set_menu_item_enabled(item: *mut c_void, enabled: bool) {
    let item = unsafe { borrowed_menu_item(item) };
    item.setEnabled(enabled);
}

pub unsafe fn set_menu_item_target(item: *mut c_void, target: *mut c_void) {
    let item = unsafe { borrowed_menu_item(item) };
    let target = unsafe { borrowed_object(target) };
    unsafe {
        item.setTarget(Some(target));
    }
}

pub unsafe fn set_menu_item_tag(item: *mut c_void, tag: isize) {
    let item = unsafe { borrowed_menu_item(item) };
    item.setTag(tag);
}

pub unsafe fn menu_item_tag(item: *mut AnyObject) -> isize {
    assert!(!item.is_null(), "NSMenuItem pointer must not be null");
    let item = unsafe { &*item.cast::<NSMenuItem>() };
    item.tag()
}

pub unsafe fn set_menu_item_title(item: *mut c_void, title: &str) {
    let item = unsafe { borrowed_menu_item(item) };
    item.setTitle(&NSString::from_str(title));
}

pub unsafe fn set_menu_item_checked(item: *mut c_void, checked: bool) {
    let item = unsafe { borrowed_menu_item(item) };
    item.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

pub unsafe fn install_status_bar_item(
    menu: *mut c_void,
    title: &str,
    tooltip: &str,
) -> Result<*mut c_void, String> {
    let mtm = main_thread_marker()?;
    let menu = unsafe { borrowed_menu(menu) };
    let status_bar = NSStatusBar::systemStatusBar();
    let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);
    status_item.setMenu(Some(menu));
    if let Some(button) = status_item.button(mtm) {
        button.setTitle(&NSString::from_str(title));
        button.setToolTip(Some(&NSString::from_str(tooltip)));
    }
    Ok(ObjcRetained::into_raw(status_item).cast())
}

pub fn open_workspace_url(url: &str) -> Result<(), String> {
    let workspace_url = NSURL::URLWithString(&NSString::from_str(url))
        .ok_or_else(|| format!("Invalid macOS workspace URL: {url}"))?;
    open_workspace_url_object(&workspace_url, url)
}

pub fn reveal_path_in_workspace(path: &Path) -> Result<(), String> {
    let url = NSURL::from_file_path(path)
        .ok_or_else(|| format!("Failed to create Finder file URL for {}", path.display()))?;
    let urls = NSArray::from_slice(&[&*url]);
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
    Ok(())
}

pub fn open_path_in_workspace(path: &Path, is_directory: bool) -> Result<(), String> {
    let url = if is_directory {
        NSURL::from_directory_path(path)
    } else {
        NSURL::from_file_path(path)
    }
    .ok_or_else(|| format!("Failed to create Finder URL for {}", path.display()))?;
    open_workspace_url_object(&url, path.display())
}

#[cfg(feature = "camera-tracking")]
pub fn local_only_camera_message(detail: &str) -> String {
    format!(
        "{detail}\nCamera tracking is local-only: frames are not stored, written to disk, or logged."
    )
}

#[cfg(feature = "screen-capture-kit")]
pub fn local_only_screen_capture_message(detail: &str) -> String {
    format!(
        "{detail}\nScreenCaptureKit probe is diagnostic-only: frames are counted, but image data is not stored, written to disk, or logged."
    )
}

fn open_workspace_url_object(url: &NSURL, label: impl std::fmt::Display) -> Result<(), String> {
    if NSWorkspace::sharedWorkspace().openURL(url) {
        Ok(())
    } else {
        Err(format!("macOS NSWorkspace refused to open {label}"))
    }
}

unsafe fn borrowed_menu_item<'a>(item: *mut c_void) -> &'a NSMenuItem {
    assert!(!item.is_null(), "NSMenuItem pointer must not be null");
    unsafe { &*item.cast::<NSMenuItem>() }
}

unsafe fn borrowed_view<'a>(view: *mut c_void) -> &'a NSView {
    assert!(!view.is_null(), "NSView pointer must not be null");
    unsafe { &*view.cast::<NSView>() }
}

unsafe fn borrowed_application<'a>(app: *mut c_void) -> &'a NSApplication {
    assert!(!app.is_null(), "NSApplication pointer must not be null");
    unsafe { &*app.cast::<NSApplication>() }
}

unsafe fn borrowed_date<'a>(date: *mut c_void) -> &'a NSDate {
    assert!(!date.is_null(), "NSDate pointer must not be null");
    unsafe { &*date.cast::<NSDate>() }
}

unsafe fn borrowed_run_loop_mode<'a>(mode: *mut c_void) -> &'a NSRunLoopMode {
    assert!(!mode.is_null(), "NSRunLoopMode pointer must not be null");
    unsafe { &*mode.cast::<NSRunLoopMode>() }
}

unsafe fn borrowed_panel<'a>(panel: *mut c_void) -> &'a NSPanel {
    assert!(!panel.is_null(), "NSPanel pointer must not be null");
    unsafe { &*panel.cast::<NSPanel>() }
}

unsafe fn borrowed_window<'a>(window: *mut c_void) -> &'a NSWindow {
    assert!(!window.is_null(), "NSWindow pointer must not be null");
    unsafe { &*window.cast::<NSWindow>() }
}

unsafe fn borrowed_text_layer<'a>(layer: *mut c_void) -> &'a CATextLayer {
    assert!(!layer.is_null(), "CATextLayer pointer must not be null");
    unsafe { &*layer.cast::<CATextLayer>() }
}

unsafe fn borrowed_layer<'a>(layer: *mut c_void) -> &'a CALayer {
    assert!(!layer.is_null(), "CALayer pointer must not be null");
    unsafe { &*layer.cast::<CALayer>() }
}

unsafe fn borrowed_cg_color<'a>(color: *mut c_void) -> &'a CGColor {
    assert!(!color.is_null(), "CGColor pointer must not be null");
    unsafe { &*color.cast::<CGColor>() }
}

unsafe fn borrowed_menu<'a>(menu: *mut c_void) -> &'a NSMenu {
    assert!(!menu.is_null(), "NSMenu pointer must not be null");
    unsafe { &*menu.cast::<NSMenu>() }
}

unsafe fn borrowed_class<'a>(class: *mut c_void) -> &'a AnyClass {
    assert!(
        !class.is_null(),
        "Objective-C class pointer must not be null"
    );
    unsafe { &*class.cast::<AnyClass>() }
}

unsafe fn borrowed_object<'a>(object: *mut c_void) -> &'a AnyObject {
    assert!(
        !object.is_null(),
        "Objective-C object pointer must not be null"
    );
    unsafe { &*object.cast::<AnyObject>() }
}

fn main_thread_marker() -> Result<MainThreadMarker, String> {
    MainThreadMarker::new().ok_or_else(|| "AppKit menu APIs must run on main thread".to_string())
}

fn selector_for_name(name: &str) -> Result<Sel, String> {
    let name = CString::new(name).map_err(|_| format!("selector contains NUL byte: {name:?}"))?;
    Ok(Sel::register(&name))
}

fn class_ptr(class: &AnyClass) -> *mut c_void {
    (class as *const AnyClass).cast_mut().cast()
}

fn cg_rect(frame: LayerFrame) -> CGRect {
    CGRect {
        origin: CGPoint {
            x: frame.x,
            y: frame.y,
        },
        size: CGSize {
            width: frame.width,
            height: frame.height,
        },
    }
}

#[cfg(feature = "metal-renderer")]
fn layer_frame(rect: CGRect) -> LayerFrame {
    LayerFrame {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

fn configure_transparent_root_layer(layer: &CALayer, background_color: &CGColor) {
    layer.setNeedsDisplayOnBoundsChange(true);
    layer.setAllowsEdgeAntialiasing(true);
    layer.setBackgroundColor(Some(background_color));
    layer.setOpaque(false);
    layer.setCornerRadius(0.0);
}

fn configure_transparent_panel(
    panel: &NSPanel,
    level: i64,
    collection_behavior: u64,
    title: &str,
    excluded_from_windows_menu: bool,
    sharing_read_only: bool,
) {
    panel.setOpaque(false);
    panel.setTitle(&NSString::from_str(title));
    if sharing_read_only {
        panel.setSharingType(NSWindowSharingType::ReadOnly);
    }
    panel.setMovableByWindowBackground(true);
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setCanHide(false);
    panel.setFloatingPanel(true);
    panel.setHidesOnDeactivate(false);
    panel.setWorksWhenModal(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setExcludedFromWindowsMenu(excluded_from_windows_menu);
    unsafe {
        set_panel_space_policy(
            (panel as *const NSPanel).cast_mut().cast(),
            level,
            collection_behavior,
        );
    }
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
}

unsafe fn set_text_layer_string(layer: &CATextLayer, text: &str) {
    let text = NSString::from_str(text);
    unsafe {
        layer.setString(Some(text.as_super()));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_menu_item, add_sublayer, application_is_active, configure_view_backed_root_layer,
        drain_pending_application_events, new_object_from_class, set_menu_item_checked,
        set_menu_item_title, set_text_layer_text, window_content_view,
    };
    use core::ffi::c_void;

    #[test]
    #[cfg(feature = "camera-tracking")]
    fn local_only_camera_message_mentions_privacy_boundary() {
        let message = super::local_only_camera_message("Probe detail.");

        assert!(message.contains("Probe detail."));
        assert!(message.contains("frames are not stored"));
        assert!(message.contains("not"));
    }

    #[test]
    #[should_panic(expected = "NSMenuItem pointer must not be null")]
    fn menu_item_helpers_reject_null_pointers() {
        unsafe {
            set_menu_item_checked(core::ptr::null_mut::<c_void>(), true);
        }
    }

    #[test]
    #[should_panic(expected = "NSMenuItem pointer must not be null")]
    fn menu_title_helper_rejects_null_pointers() {
        unsafe {
            set_menu_item_title(core::ptr::null_mut::<c_void>(), "Title");
        }
    }

    #[test]
    #[should_panic(expected = "NSMenu pointer must not be null")]
    fn add_menu_item_rejects_null_menu_pointer() {
        unsafe {
            add_menu_item(
                core::ptr::null_mut::<c_void>(),
                core::ptr::null_mut::<c_void>(),
            );
        }
    }

    #[test]
    #[should_panic(expected = "Objective-C class pointer must not be null")]
    fn new_object_from_class_rejects_null_class_pointer() {
        unsafe {
            let _ = new_object_from_class(core::ptr::null_mut::<c_void>());
        }
    }

    #[test]
    #[should_panic(expected = "CATextLayer pointer must not be null")]
    fn set_text_layer_text_rejects_null_pointer() {
        unsafe {
            set_text_layer_text(core::ptr::null_mut::<c_void>(), "text");
        }
    }

    #[test]
    #[should_panic(expected = "CALayer pointer must not be null")]
    fn add_sublayer_rejects_null_parent_pointer() {
        unsafe {
            add_sublayer(
                core::ptr::null_mut::<c_void>(),
                core::ptr::null_mut::<c_void>(),
            );
        }
    }

    #[test]
    #[should_panic(expected = "NSView pointer must not be null")]
    fn root_layer_helper_rejects_null_content_view_pointer() {
        unsafe {
            let _ = configure_view_backed_root_layer(
                core::ptr::null_mut::<c_void>(),
                core::ptr::null_mut::<c_void>(),
            );
        }
    }

    #[test]
    #[should_panic(expected = "NSWindow pointer must not be null")]
    fn window_content_view_rejects_null_window_pointer() {
        unsafe {
            let _ = window_content_view(core::ptr::null_mut::<c_void>());
        }
    }

    #[test]
    #[should_panic(expected = "NSApplication pointer must not be null")]
    fn application_is_active_rejects_null_app_pointer() {
        unsafe {
            let _ = application_is_active(core::ptr::null_mut::<c_void>());
        }
    }

    #[test]
    #[should_panic(expected = "NSApplication pointer must not be null")]
    fn drain_pending_events_rejects_null_app_pointer() {
        unsafe {
            drain_pending_application_events(
                core::ptr::null_mut::<c_void>(),
                core::ptr::null_mut::<c_void>(),
                core::ptr::null_mut::<c_void>(),
                u64::MAX,
            );
        }
    }
}
