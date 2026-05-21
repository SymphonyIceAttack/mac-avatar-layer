#![cfg(target_os = "macos")]

use crate::config::CameraConfig;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraStatus {
    Disabled,
    WaitingForPermission,
    PermissionDenied,
    NoCamera,
    BackendPending,
    Failed,
}

impl CameraStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::WaitingForPermission => "waiting for permission",
            Self::PermissionDenied => "permission denied",
            Self::NoCamera => "no camera",
            Self::BackendPending => "backend pending",
            Self::Failed => "failed",
        }
    }
}

pub struct CameraInput {
    status: CameraStatus,
    diagnostic: Option<String>,
}

impl CameraInput {
    pub fn from_config(config: &CameraConfig) -> Self {
        if !config.enabled {
            return Self {
                status: CameraStatus::Disabled,
                diagnostic: None,
            };
        }

        #[cfg(feature = "camera-tracking")]
        {
            return Self::from_native_probe(config);
        }

        #[cfg(not(feature = "camera-tracking"))]
        {
            eprintln!(
                "Camera tracking is enabled in config, but this build was compiled without `--features camera-tracking`. The avatar will continue without camera tracking."
            );
            Self {
                status: CameraStatus::BackendPending,
                diagnostic: Some("build missing camera-tracking feature".to_string()),
            }
        }
    }

    #[cfg(feature = "camera-tracking")]
    fn from_native_probe(config: &CameraConfig) -> Self {
        match crate::macos_camera::CameraProbe::detect(config) {
            Ok(probe) => {
                if let Some(message) = probe.diagnostic.as_deref() {
                    eprintln!("{message}");
                }

                Self {
                    status: probe.status,
                    diagnostic: probe.diagnostic,
                }
            }
            Err(error) => {
                eprintln!("Camera tracking setup failed: {error}");
                Self {
                    status: CameraStatus::Failed,
                    diagnostic: Some(error),
                }
            }
        }
    }

    pub fn status(&self) -> CameraStatus {
        self.status
    }

    pub fn status_label(&self) -> &'static str {
        self.status.label()
    }

    pub fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_reports_disabled_without_side_effects() {
        let config = CameraConfig::default();
        let input = CameraInput::from_config(&config);

        assert_eq!(input.status(), CameraStatus::Disabled);
        assert_eq!(input.status_label(), "disabled");
        assert_eq!(input.diagnostic(), None);
    }

    #[cfg(not(feature = "camera-tracking"))]
    #[test]
    fn enabled_config_reports_backend_pending_until_native_backend_lands() {
        let config = CameraConfig {
            enabled: true,
            ..CameraConfig::default()
        };
        let input = CameraInput::from_config(&config);

        assert_eq!(input.status(), CameraStatus::BackendPending);
        assert_eq!(input.status_label(), "backend pending");
        assert_eq!(
            input.diagnostic(),
            Some("build missing camera-tracking feature")
        );
    }

    #[test]
    fn status_labels_match_diagnostics_surface() {
        assert_eq!(
            CameraStatus::WaitingForPermission.label(),
            "waiting for permission"
        );
        assert_eq!(CameraStatus::PermissionDenied.label(), "permission denied");
        assert_eq!(CameraStatus::NoCamera.label(), "no camera");
        assert_eq!(CameraStatus::Failed.label(), "failed");
    }
}
