#![cfg(all(target_os = "macos", feature = "system-extension-activation"))]

use dispatch2::DispatchQueue;
use objc2_foundation::NSString;
use objc2_system_extensions::{OSSystemExtensionManager, OSSystemExtensionRequest};

pub const CAMERA_EXTENSION_BUNDLE_ID: &str =
    "io.github.symphonyiceattack.mac-avatar-layer.CameraExtension";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[allow(dead_code)]
pub enum SystemExtensionRequestKind {
    Activate,
    Deactivate,
}

impl SystemExtensionRequestKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Activate => "activate",
            Self::Deactivate => "deactivate",
        }
    }
}

pub fn submit_camera_extension_request(kind: SystemExtensionRequestKind) -> Result<(), String> {
    let identifier = NSString::from_str(CAMERA_EXTENSION_BUNDLE_ID);
    let queue = DispatchQueue::main();
    let request = unsafe {
        match kind {
            SystemExtensionRequestKind::Activate => {
                OSSystemExtensionRequest::activationRequestForExtension_queue(&identifier, queue)
            }
            SystemExtensionRequestKind::Deactivate => {
                OSSystemExtensionRequest::deactivationRequestForExtension_queue(&identifier, queue)
            }
        }
    };
    let manager = unsafe { OSSystemExtensionManager::sharedManager() };
    unsafe {
        manager.submitRequest(&request);
    }
    Ok(())
}

pub fn activation_note() -> &'static str {
    "System Extension activation requires the app bundle to live in /Applications with Apple Developer Program signing/provisioning."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_kind_labels_are_stable_for_logs() {
        assert_eq!(SystemExtensionRequestKind::Activate.label(), "activate");
        assert_eq!(SystemExtensionRequestKind::Deactivate.label(), "deactivate");
    }

    #[test]
    fn camera_extension_bundle_id_matches_planned_extension() {
        assert_eq!(
            CAMERA_EXTENSION_BUNDLE_ID,
            "io.github.symphonyiceattack.mac-avatar-layer.CameraExtension"
        );
        assert!(activation_note().contains("/Applications"));
    }
}
