#![cfg(all(target_os = "macos", feature = "iosurface-output"))]

use metal::foreign_types::ForeignType;
use metal::{
    Device, MTLPixelFormat, MTLResourceOptions, MTLTextureType, MTLTextureUsage, Texture,
    TextureDescriptor,
};
use objc2::runtime::Sel;
use objc2::sel;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFType};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use std::ffi::c_void;

const BGRA_FOURCC: i32 = i32::from_be_bytes(*b"BGRA");

unsafe extern "C" {
    fn objc_msgSend();
}

pub struct IosurfaceOutput {
    surface: CFRetained<IOSurfaceRef>,
    texture: Texture,
    width: u64,
    height: u64,
    frames: u64,
}

impl IosurfaceOutput {
    pub fn new(device: &Device, width: u64, height: u64) -> Result<Self, String> {
        let width = width.max(1);
        let height = height.max(1);
        let bytes_per_element = 4_u64;
        let bytes_per_row = width.saturating_mul(bytes_per_element);
        let surface = create_iosurface(width, height, bytes_per_row, bytes_per_element)?;
        let texture = create_iosurface_texture(device, &surface, width, height)?;
        println!(
            "renderer_event=iosurface_output_created id={} width={} height={} bytes_per_row={}",
            surface.id(),
            width,
            height,
            bytes_per_row
        );
        Ok(Self {
            surface,
            texture,
            width,
            height,
            frames: 0,
        })
    }

    pub fn texture(&self) -> &Texture {
        &self.texture
    }

    pub fn id(&self) -> u32 {
        self.surface.id()
    }

    pub fn width(&self) -> u64 {
        self.width
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn record_frame(&mut self) {
        self.frames = self.frames.saturating_add(1);
        if self.frames % 120 == 1 {
            println!(
                "renderer_event=iosurface_output_frame id={} frames={} width={} height={}",
                self.id(),
                self.frames,
                self.width,
                self.height
            );
        }
    }
}

fn create_iosurface(
    width: u64,
    height: u64,
    bytes_per_row: u64,
    bytes_per_element: u64,
) -> Result<CFRetained<IOSurfaceRef>, String> {
    let width_value = CFNumber::new_i64(width as i64);
    let height_value = CFNumber::new_i64(height as i64);
    let bytes_per_row_value = CFNumber::new_i64(bytes_per_row as i64);
    let bytes_per_element_value = CFNumber::new_i64(bytes_per_element as i64);
    let pixel_format_value = CFNumber::new_i32(BGRA_FOURCC);

    let keys: [&CFType; 5] = unsafe {
        [
            kIOSurfaceWidth.as_ref(),
            kIOSurfaceHeight.as_ref(),
            kIOSurfaceBytesPerRow.as_ref(),
            kIOSurfaceBytesPerElement.as_ref(),
            kIOSurfacePixelFormat.as_ref(),
        ]
    };
    let properties = CFDictionary::<CFType, CFType>::from_slices(
        &keys,
        &[
            width_value.as_ref(),
            height_value.as_ref(),
            bytes_per_row_value.as_ref(),
            bytes_per_element_value.as_ref(),
            pixel_format_value.as_ref(),
        ],
    );

    unsafe { IOSurfaceRef::new(properties.as_ref()) }
        .ok_or_else(|| "IOSurfaceCreate returned null".to_string())
}

fn create_iosurface_texture(
    device: &Device,
    surface: &IOSurfaceRef,
    width: u64,
    height: u64,
) -> Result<Texture, String> {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    descriptor.set_width(width.max(1));
    descriptor.set_height(height.max(1));
    descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    descriptor.set_resource_options(MTLResourceOptions::StorageModePrivate);

    let texture = unsafe {
        new_texture_with_iosurface(device, &descriptor, surface)
            .ok_or_else(|| "newTextureWithDescriptor:iosurface:plane: returned nil".to_string())?
    };
    Ok(texture)
}

unsafe fn new_texture_with_iosurface(
    device: &Device,
    descriptor: &TextureDescriptor,
    surface: &IOSurfaceRef,
) -> Option<Texture> {
    let function: extern "C" fn(
        *mut c_void,
        Sel,
        *mut c_void,
        *const IOSurfaceRef,
        usize,
    ) -> *mut c_void = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    let texture = function(
        device.as_ptr().cast(),
        sel!(newTextureWithDescriptor:iosurface:plane:),
        descriptor.as_ptr().cast(),
        surface,
        0,
    );
    if texture.is_null() {
        None
    } else {
        Some(unsafe { Texture::from_ptr(texture.cast()) })
    }
}
