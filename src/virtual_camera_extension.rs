#![cfg(all(target_os = "macos", feature = "virtual-camera-extension"))]

pub const CAMERA_LOCALIZED_NAME: &str = "VTube Studio RS Camera";
pub const EXTENSION_BUNDLE_ID: &str = "rs.vtube-studio.dev.CameraExtension";
pub const EXTENSION_MACH_SERVICE: &str = "rs.vtube-studio.dev.CameraExtension";
pub const APP_GROUP_ID: &str = "group.rs.vtube-studio.dev";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct VirtualCameraExtensionDescriptor {
    pub camera_name: &'static str,
    pub bundle_id: &'static str,
    pub mach_service: &'static str,
    pub app_group_id: &'static str,
    pub frame_source: &'static str,
}

impl VirtualCameraExtensionDescriptor {
    pub const fn current() -> Self {
        Self {
            camera_name: CAMERA_LOCALIZED_NAME,
            bundle_id: EXTENSION_BUNDLE_ID,
            mach_service: EXTENSION_MACH_SERVICE,
            app_group_id: APP_GROUP_ID,
            frame_source: "target/internal-output/iosurface.json",
        }
    }
}

pub fn objc2_binding_summary() -> &'static str {
    let _ = std::any::type_name::<objc2_core_media_io::CMIOExtensionProvider>();
    let _ = std::any::type_name::<objc2_core_media_io::CMIOExtensionDevice>();
    let _ = std::any::type_name::<objc2_core_media_io::CMIOExtensionStream>();
    let _ = std::any::type_name::<objc2_core_video::CVPixelBuffer>();
    "objc2-core-media-io + objc2-core-video"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_names_the_system_camera_source() {
        let descriptor = VirtualCameraExtensionDescriptor::current();
        assert_eq!(descriptor.camera_name, "VTube Studio RS Camera");
        assert!(descriptor.bundle_id.ends_with(".CameraExtension"));
        assert_eq!(descriptor.bundle_id, descriptor.mach_service);
        assert!(descriptor.app_group_id.starts_with("group."));
        assert!(descriptor.frame_source.ends_with("iosurface.json"));
    }

    #[test]
    fn objc2_bindings_are_resolved() {
        assert_eq!(
            objc2_binding_summary(),
            "objc2-core-media-io + objc2-core-video"
        );
    }
}
