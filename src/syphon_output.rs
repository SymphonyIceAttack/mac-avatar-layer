#![cfg(all(target_os = "macos", feature = "syphon-output"))]

use core::ffi::c_void;

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::NSString;

pub struct SyphonOutput {
    server: *mut AnyObject,
    name: String,
    width: u64,
    height: u64,
}

impl SyphonOutput {
    pub unsafe fn new(device: *mut c_void, name: &str) -> Result<Self, String> {
        if device.is_null() {
            return Err("Syphon output cannot start with a null Metal device".to_string());
        }
        let class = AnyClass::get(c"SyphonMetalServer").ok_or_else(|| {
            "SyphonMetalServer class not found. Check Syphon.framework.".to_string()
        })?;
        let server_name = NSString::from_str(name);
        let allocated: *mut AnyObject = unsafe { msg_send![class, alloc] };
        if allocated.is_null() {
            return Err("SyphonMetalServer allocation returned nil".to_string());
        }
        let server: *mut AnyObject = unsafe {
            msg_send![
                allocated,
                initWithName: &*server_name,
                device: device.cast::<AnyObject>(),
                options: core::ptr::null_mut::<AnyObject>()
            ]
        };
        if server.is_null() {
            return Err("SyphonMetalServer init returned nil".to_string());
        }
        Ok(Self {
            server,
            name: name.to_string(),
            width: 0,
            height: 0,
        })
    }

    pub unsafe fn publish(
        &mut self,
        texture: *mut c_void,
        command_buffer: *mut c_void,
        width: u64,
        height: u64,
    ) -> Result<(), String> {
        if texture.is_null() {
            return Err("Syphon publish received a null Metal texture".to_string());
        }
        if command_buffer.is_null() {
            return Err("Syphon publish received a null Metal command buffer".to_string());
        }
        if self.width != width || self.height != height {
            println!(
                "renderer_event=syphon_output_resized name=\"{}\" width={} height={}",
                self.name, width, height
            );
            self.width = width;
            self.height = height;
        }
        let region = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: width as f64,
                height: height as f64,
            },
        };
        unsafe {
            let _: () = msg_send![
                self.server,
                publishFrameTexture: texture.cast::<AnyObject>(),
                onCommandBuffer: command_buffer.cast::<AnyObject>(),
                imageRegion: region,
                flipped: Bool::NO
            ];
        }
        Ok(())
    }
}

impl Drop for SyphonOutput {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.server, stop];
            let _: () = msg_send![self.server, release];
        }
    }
}
