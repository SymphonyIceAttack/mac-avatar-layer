#![cfg(all(target_os = "macos", feature = "camera-tracking"))]

use crate::camera_input::CameraStatus;
use crate::config::CameraConfig;
use std::ffi::CString;
use std::os::raw::{c_char, c_void};

type Id = *mut c_void;
type Sel = *mut c_void;

const NIL: Id = std::ptr::null_mut();

const AV_AUTHORIZATION_STATUS_NOT_DETERMINED: i64 = 0;
const AV_AUTHORIZATION_STATUS_RESTRICTED: i64 = 1;
const AV_AUTHORIZATION_STATUS_DENIED: i64 = 2;
const AV_AUTHORIZATION_STATUS_AUTHORIZED: i64 = 3;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Vision", kind = "framework")]
unsafe extern "C" {}

unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn objc_msgSend();
    fn sel_registerName(name: *const c_char) -> Sel;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CameraProbe {
    pub status: CameraStatus,
    pub diagnostic: Option<String>,
}

impl CameraProbe {
    pub fn detect(config: &CameraConfig) -> Result<Self, String> {
        detect_camera(config)
    }
}

fn detect_camera(config: &CameraConfig) -> Result<CameraProbe, String> {
    let media_type_video = ns_string("vide")?;
    let capture_device = class("AVCaptureDevice")?;
    let auth_status = msg_id_id_i64(
        capture_device,
        "authorizationStatusForMediaType:",
        media_type_video,
    );

    match auth_status {
        AV_AUTHORIZATION_STATUS_NOT_DETERMINED => {
            return Ok(CameraProbe {
                status: CameraStatus::WaitingForPermission,
                diagnostic: Some(camera_setup_message(
                    "Camera permission has not been granted yet. macOS may ask for access once capture starts; until then the avatar keeps rendering without camera tracking.",
                )),
            });
        }
        AV_AUTHORIZATION_STATUS_RESTRICTED | AV_AUTHORIZATION_STATUS_DENIED => {
            return Ok(CameraProbe {
                status: CameraStatus::PermissionDenied,
                diagnostic: Some(camera_setup_message(
                    "Camera permission is denied or restricted. Enable camera access for the terminal/app that launches vtube-studio-rs in System Settings > Privacy & Security > Camera.",
                )),
            });
        }
        AV_AUTHORIZATION_STATUS_AUTHORIZED => {}
        other => {
            return Ok(CameraProbe {
                status: CameraStatus::Failed,
                diagnostic: Some(format!(
                    "Unexpected AVFoundation camera authorization status: {other}"
                )),
            });
        }
    }

    let device = if config.device.trim().is_empty() {
        default_video_device(capture_device, media_type_video)?
    } else {
        named_video_device(capture_device, media_type_video, config.device.trim())?
    };

    if device == NIL {
        return Ok(CameraProbe {
            status: CameraStatus::NoCamera,
            diagnostic: Some(camera_setup_message(
                "No matching macOS camera was found. Check that a webcam is connected and available to AVFoundation.",
            )),
        });
    }

    Ok(CameraProbe {
        status: CameraStatus::BackendPending,
        diagnostic: Some(camera_setup_message(
            "Camera permission and device probing succeeded. Frame capture and Vision landmarks are the next backend step, so tracking remains inactive for now.",
        )),
    })
}

fn default_video_device(capture_device: Id, media_type_video: Id) -> Result<Id, String> {
    Ok(msg_id_id_id(
        capture_device,
        "defaultDeviceWithMediaType:",
        media_type_video,
    ))
}

fn named_video_device(
    capture_device: Id,
    media_type_video: Id,
    requested_name: &str,
) -> Result<Id, String> {
    let devices = msg_id_id_id(capture_device, "devicesWithMediaType:", media_type_video);
    if devices == NIL {
        return Ok(NIL);
    }

    let count = msg_id_usize(devices, "count");
    for index in 0..count {
        let device = msg_id_usize_id(devices, "objectAtIndex:", index);
        if device == NIL {
            continue;
        }

        let localized_name = msg_id(device, "localizedName");
        if ns_string_to_string(localized_name).as_deref() == Some(requested_name) {
            return Ok(device);
        }
    }

    Ok(NIL)
}

fn camera_setup_message(detail: &str) -> String {
    format!(
        "{detail}\nCamera tracking is local-only: frames are not stored, written to disk, or logged."
    )
}

fn class(name: &str) -> Result<Id, String> {
    let name = CString::new(name).map_err(|error| error.to_string())?;
    let class = unsafe { objc_getClass(name.as_ptr()) };
    if class == NIL {
        Err(format!(
            "Objective-C class not found: {}",
            name.to_string_lossy()
        ))
    } else {
        Ok(class)
    }
}

fn selector(name: &str) -> Sel {
    let name = CString::new(name).expect("selector names must not contain NUL bytes");
    unsafe { sel_registerName(name.as_ptr()) }
}

fn ns_string(value: &str) -> Result<Id, String> {
    let value = CString::new(value).map_err(|error| error.to_string())?;
    let string = msg_id(class("NSString")?, "alloc");
    Ok(msg_id_cstr_id(
        string,
        "initWithUTF8String:",
        value.as_ptr(),
    ))
}

fn ns_string_to_string(value: Id) -> Option<String> {
    if value == NIL {
        return None;
    }

    let bytes = msg_id(value, "UTF8String") as *const c_char;
    if bytes.is_null() {
        return None;
    }

    Some(
        unsafe { std::ffi::CStr::from_ptr(bytes) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn msg_id(receiver: Id, selector_name: &str) -> Id {
    let function: extern "C" fn(Id, Sel) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    function(receiver, selector(selector_name))
}

fn msg_id_id_id(receiver: Id, selector_name: &str, value: Id) -> Id {
    let function: extern "C" fn(Id, Sel, Id) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    function(receiver, selector(selector_name), value)
}

fn msg_id_id_i64(receiver: Id, selector_name: &str, value: Id) -> i64 {
    let function: extern "C" fn(Id, Sel, Id) -> i64 =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    function(receiver, selector(selector_name), value)
}

fn msg_id_cstr_id(receiver: Id, selector_name: &str, value: *const c_char) -> Id {
    let function: extern "C" fn(Id, Sel, *const c_char) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    function(receiver, selector(selector_name), value)
}

fn msg_id_usize(receiver: Id, selector_name: &str) -> usize {
    let function: extern "C" fn(Id, Sel) -> usize =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    function(receiver, selector(selector_name))
}

fn msg_id_usize_id(receiver: Id, selector_name: &str, value: usize) -> Id {
    let function: extern "C" fn(Id, Sel, usize) -> Id =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    function(receiver, selector(selector_name), value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_setup_message_mentions_local_privacy_boundary() {
        let message = camera_setup_message("Probe detail.");

        assert!(message.contains("Probe detail."));
        assert!(message.contains("frames are not stored"));
        assert!(message.contains("not"));
    }
}
