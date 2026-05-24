#![cfg(all(target_os = "macos", feature = "iosurface-output"))]
#![allow(deprecated)]

use metal::foreign_types::ForeignType;
use metal::{
    Device, MTLPixelFormat, MTLResourceOptions, MTLTextureType, MTLTextureUsage, Texture,
    TextureDescriptor,
};
use objc2::runtime::Sel;
use objc2::sel;
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFType};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight,
    kIOSurfaceIsGlobal, kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use serde::Serialize;
use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const BGRA_FOURCC: i32 = i32::from_be_bytes(*b"BGRA");

unsafe extern "C" {
    fn objc_msgSend();
}

pub struct IosurfaceOutput {
    surface: CFRetained<IOSurfaceRef>,
    texture: Texture,
    manifest_path: PathBuf,
    width: u64,
    height: u64,
    frames: u64,
}

impl IosurfaceOutput {
    pub fn new(
        device: &Device,
        width: u64,
        height: u64,
        manifest_path: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let width = width.max(1);
        let height = height.max(1);
        let manifest_path = manifest_path.into();
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
        let output = Self {
            surface,
            texture,
            manifest_path,
            width,
            height,
            frames: 0,
        };
        output.write_manifest(bytes_per_row)?;
        Ok(output)
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
        if self.frames % 30 == 1 {
            let bytes_per_row = self.width.saturating_mul(4);
            if let Err(error) = self.write_manifest(bytes_per_row) {
                eprintln!(
                    "renderer_event=iosurface_manifest_write_failed path={} error={}",
                    self.manifest_path.display(),
                    error
                );
            }
        }
    }

    fn write_manifest(&self, bytes_per_row: u64) -> Result<(), String> {
        if self.manifest_path.as_os_str().is_empty() {
            return Ok(());
        }
        write_manifest(
            &self.manifest_path,
            &IosurfaceManifest {
                schema_version: 1,
                producer: "vtube-studio-rs",
                producer_kind: "iosurface",
                pid: std::process::id(),
                iosurface_id: self.id(),
                width: self.width,
                height: self.height,
                bytes_per_row,
                bytes_per_element: 4,
                pixel_format: "BGRA8Unorm",
                color_space: "sRGB",
                frame_rate: 60,
                frame_duration_num: 1,
                frame_duration_den: 60,
                intended_consumer: "coremediaio-camera-extension",
                frames: self.frames,
                updated_unix_ms: unix_time_millis(),
                note: "IOSurface producer handoff manifest for the in-project CoreMediaIO camera extension.",
            },
        )
    }
}

#[derive(Serialize)]
struct IosurfaceManifest {
    schema_version: u32,
    producer: &'static str,
    producer_kind: &'static str,
    pid: u32,
    iosurface_id: u32,
    width: u64,
    height: u64,
    bytes_per_row: u64,
    bytes_per_element: u64,
    pixel_format: &'static str,
    color_space: &'static str,
    frame_rate: u32,
    frame_duration_num: u32,
    frame_duration_den: u32,
    intended_consumer: &'static str,
    frames: u64,
    updated_unix_ms: u128,
    note: &'static str,
}

fn write_manifest(path: &Path, manifest: &IosurfaceManifest) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let temp_path = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("failed to serialize IOSurface manifest: {error}"))?;
    fs::write(&temp_path, text)
        .map_err(|error| format!("failed to write {}: {error}", temp_path.display()))?;
    fs::rename(&temp_path, path).map_err(|error| {
        format!(
            "failed to replace {} with {}: {error}",
            path.display(),
            temp_path.display()
        )
    })
}

fn unix_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
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
    let is_global_value = CFBoolean::new(true);

    let keys: [&CFType; 6] = unsafe {
        [
            kIOSurfaceWidth.as_ref(),
            kIOSurfaceHeight.as_ref(),
            kIOSurfaceBytesPerRow.as_ref(),
            kIOSurfaceBytesPerElement.as_ref(),
            kIOSurfacePixelFormat.as_ref(),
            kIOSurfaceIsGlobal.as_ref(),
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
            is_global_value.as_ref(),
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
