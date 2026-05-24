use serde::Deserialize;
use std::fs;
use std::path::Path;

const DEVELOPMENT_CONFIG_PATH: &str = "vtube-studio-rs.dev.toml";
const BUILD_CONFIG_PATH: &str = "vtube-studio-rs.build.toml";
const DEVELOPMENT_EXAMPLE_CONFIG_PATH: &str = "vtube-studio-rs.dev.example.toml";
const BUILD_EXAMPLE_CONFIG_PATH: &str = "vtube-studio-rs.build.example.toml";
const DEFAULT_MODEL_PATH: &str = "public/model/0.model3.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub app: AppRuntimeConfig,
    pub output: OutputConfig,
    pub capture: CaptureConfig,
    pub model: ModelConfig,
    pub diagnostics: DiagnosticsConfig,
    pub renderer: RendererConfig,
    pub motion: MotionConfig,
    pub input: InputConfig,
    pub overrides: OverridesConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self, String> {
        let path = Path::new(default_config_path());
        let example_path = Path::new(default_example_config_path());
        if ensure_config_file_from_example(path, example_path)? {
            println!(
                "Created local config: {} from {}",
                path.display(),
                example_path.display()
            );
        }
        if path.is_file() {
            return Self::load_from_path(path);
        }

        let mut config = Self::default();
        config.app.runtime_profile = default_runtime_profile();
        Ok(config)
    }

    pub(crate) fn load_from_path(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let config = toml::from_str(&text)
            .map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;
        println!("Loaded config: {}", path.display());
        Ok(config)
    }

    pub fn resolved_model_path(&self, cli_model_path: Option<&str>) -> String {
        cli_model_path
            .or(self.model.path.as_deref())
            .unwrap_or(DEFAULT_MODEL_PATH)
            .to_string()
    }
}

pub fn active_config_path() -> &'static str {
    if cfg!(debug_assertions) {
        DEVELOPMENT_CONFIG_PATH
    } else {
        BUILD_CONFIG_PATH
    }
}

pub fn active_select_model_flag() -> &'static str {
    if cfg!(debug_assertions) {
        "--dev"
    } else {
        "--build"
    }
}

fn default_config_path() -> &'static str {
    active_config_path()
}

fn default_example_config_path() -> &'static str {
    if cfg!(debug_assertions) {
        DEVELOPMENT_EXAMPLE_CONFIG_PATH
    } else {
        BUILD_EXAMPLE_CONFIG_PATH
    }
}

fn ensure_config_file_from_example(
    config_path: &Path,
    example_path: &Path,
) -> Result<bool, String> {
    if config_path.is_file() || !example_path.is_file() {
        return Ok(false);
    }

    if let Some(parent) = config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }
    fs::copy(example_path, config_path).map_err(|error| {
        format!(
            "Failed to create local config {} from {}: {error}",
            config_path.display(),
            example_path.display()
        )
    })?;
    Ok(true)
}

fn default_runtime_profile() -> RuntimeProfile {
    if cfg!(debug_assertions) {
        RuntimeProfile::Development
    } else {
        RuntimeProfile::Release
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app: AppRuntimeConfig::default(),
            output: OutputConfig::default(),
            capture: CaptureConfig::default(),
            model: ModelConfig::default(),
            diagnostics: DiagnosticsConfig::default(),
            renderer: RendererConfig::default(),
            motion: MotionConfig::default(),
            input: InputConfig::default(),
            overrides: OverridesConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeProfile {
    Development,
    Release,
}

impl RuntimeProfile {
    pub fn default_enable_msaa(self) -> bool {
        matches!(self, Self::Development)
    }

    #[cfg(any(test, feature = "metal-renderer"))]
    pub fn default_log_renderer_events(self) -> bool {
        matches!(self, Self::Development)
    }
}

impl Default for RuntimeProfile {
    fn default() -> Self {
        Self::Development
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppRuntimeConfig {
    pub runtime_profile: RuntimeProfile,
    pub window_level: String,
    pub window_width: f64,
    pub window_height: f64,
    pub window_capture_friendly: bool,
}

impl Default for AppRuntimeConfig {
    fn default() -> Self {
        Self {
            runtime_profile: RuntimeProfile::default(),
            window_level: "screen_saver".to_string(),
            window_width: 360.0,
            window_height: 480.0,
            window_capture_friendly: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub mode: String,
    pub internal: InternalOutputConfig,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            mode: "window".to_string(),
            internal: InternalOutputConfig::default(),
        }
    }
}

impl OutputConfig {
    pub fn mode(&self) -> OutputMode {
        OutputMode::from_config(&self.mode)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OutputMode {
    Window,
    Internal,
}

impl OutputMode {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "internal" | "obs_internal" | "offscreen" | "offscreen_internal" => Self::Internal,
            _ => Self::Window,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Internal => "internal",
        }
    }

    pub fn uses_window(self) -> bool {
        matches!(self, Self::Window)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct InternalOutputConfig {
    pub width: f64,
    pub height: f64,
    pub producer: String,
    pub manifest_path: String,
}

impl Default for InternalOutputConfig {
    fn default() -> Self {
        Self {
            width: 1080.0,
            height: 1080.0,
            producer: "none".to_string(),
            manifest_path: "target/internal-output/iosurface.json".to_string(),
        }
    }
}

impl InternalOutputConfig {
    #[allow(dead_code)]
    pub fn producer(&self) -> InternalOutputProducer {
        InternalOutputProducer::from_config(&self.producer)
    }

    #[allow(dead_code)]
    pub fn manifest_path(&self) -> &str {
        self.manifest_path.trim()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InternalOutputProducer {
    None,
    Iosurface,
}

impl InternalOutputProducer {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "iosurface" | "io_surface" => Self::Iosurface,
            _ => Self::None,
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Iosurface => "iosurface",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub screen_capture_kit: ScreenCaptureKitConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ScreenCaptureKitConfig {
    pub enabled: Option<bool>,
    pub target_fps: u32,
    pub log_interval_seconds: f32,
    pub stalled_after_seconds: f32,
}

impl Default for ScreenCaptureKitConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            target_fps: 10,
            log_interval_seconds: 2.0,
            stalled_after_seconds: 2.0,
        }
    }
}

impl ScreenCaptureKitConfig {
    pub fn enabled(&self, runtime_profile: RuntimeProfile) -> bool {
        self.enabled
            .unwrap_or_else(|| matches!(runtime_profile, RuntimeProfile::Development))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DiagnosticsConfig {
    pub show: bool,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self { show: true }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RendererConfig {
    pub disable_masks: bool,
    pub high_precision_masks: bool,
    pub enable_msaa: Option<bool>,
    pub log_events: Option<bool>,
    pub atlas_mipmaps: bool,
    pub atlas_anisotropy: u64,
    pub debug_texture_mode: Option<String>,
    pub hidden_drawables: Vec<String>,
    pub hidden_parts: Vec<String>,
    pub only_drawables: Vec<String>,
    pub only_parts: Vec<String>,
    pub highlight_drawables: Vec<String>,
    pub highlight_parts: Vec<String>,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            disable_masks: false,
            high_precision_masks: false,
            enable_msaa: None,
            log_events: None,
            atlas_mipmaps: false,
            atlas_anisotropy: 1,
            debug_texture_mode: None,
            hidden_drawables: Vec::new(),
            hidden_parts: Vec::new(),
            only_drawables: Vec::new(),
            only_parts: Vec::new(),
            highlight_drawables: Vec::new(),
            highlight_parts: Vec::new(),
        }
    }
}

impl RendererConfig {
    pub fn enable_msaa(&self, runtime_profile: RuntimeProfile) -> bool {
        self.enable_msaa
            .unwrap_or_else(|| runtime_profile.default_enable_msaa())
    }

    #[cfg(any(test, feature = "metal-renderer"))]
    pub fn log_events(&self, runtime_profile: RuntimeProfile) -> bool {
        self.log_events
            .unwrap_or_else(|| runtime_profile.default_log_renderer_events())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MotionConfig {
    pub expression: Option<String>,
    pub blink_interval: f32,
    pub blink_duration: f32,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            expression: None,
            blink_interval: 3.8,
            blink_duration: 0.18,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    pub mouse: MouseConfig,
    pub microphone: MicrophoneConfig,
    pub camera: CameraConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MouseConfig {
    pub enabled: bool,
    pub coordinate_space: String,
    pub smoothing: f32,
    pub dead_zone: f32,
    pub invert_x: bool,
    pub invert_y: bool,
    pub eye_x_range: f32,
    pub eye_y_range: f32,
    pub angle_x_degrees: f32,
    pub angle_y_degrees: f32,
    pub angle_z_degrees: f32,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            coordinate_space: "screen".to_string(),
            smoothing: 10.0,
            dead_zone: 0.02,
            invert_x: false,
            invert_y: false,
            eye_x_range: 1.0,
            eye_y_range: 1.0,
            angle_x_degrees: 30.0,
            angle_y_degrees: 22.0,
            angle_z_degrees: -12.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MicrophoneConfig {
    pub enabled: bool,
    pub parameter: String,
    pub gain: f32,
    pub noise_gate: f32,
    pub response_curve: f32,
    pub smoothing: f32,
    pub attack: f32,
    pub release: f32,
    pub min_open: f32,
    pub max_open: f32,
}

impl Default for MicrophoneConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            parameter: "ParamMouthOpenY".to_string(),
            gain: 10.0,
            noise_gate: 0.008,
            response_curve: 0.6,
            smoothing: 18.0,
            attack: 32.0,
            release: 10.0,
            min_open: 0.0,
            max_open: 1.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CameraConfig {
    pub enabled: bool,
    pub device: String,
    pub target_fps: u32,
    pub pose_mode: String,
    pub smoothing: f32,
    pub dead_zone: f32,
    pub invert_x: bool,
    pub invert_y: bool,
    pub face_x_offset: f32,
    pub face_y_offset: f32,
    pub gaze_x_offset: f32,
    pub gaze_y_offset: f32,
    pub roll_offset: f32,
    pub angle_x_degrees: f32,
    pub angle_y_degrees: f32,
    pub angle_z_degrees: f32,
    pub eye_x_range: f32,
    pub eye_y_range: f32,
    pub mouth_enabled: bool,
    pub mouth_gain: f32,
    pub mouth_open_offset: f32,
    pub mouth_min_open: f32,
    pub mouth_max_open: f32,
    pub mouth_combine: String,
    pub blink_from_camera: bool,
    pub blink_close_threshold: f32,
    pub blink_open_threshold: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            device: String::new(),
            target_fps: 30,
            pose_mode: "camera_when_available".to_string(),
            smoothing: 12.0,
            dead_zone: 0.03,
            invert_x: true,
            invert_y: false,
            face_x_offset: 0.0,
            face_y_offset: 0.0,
            gaze_x_offset: 0.0,
            gaze_y_offset: 0.0,
            roll_offset: 0.0,
            angle_x_degrees: 30.0,
            angle_y_degrees: 22.0,
            angle_z_degrees: 12.0,
            eye_x_range: 1.0,
            eye_y_range: 1.0,
            mouth_enabled: true,
            mouth_gain: 1.4,
            mouth_open_offset: 0.0,
            mouth_min_open: 0.0,
            mouth_max_open: 1.0,
            mouth_combine: "max".to_string(),
            blink_from_camera: false,
            blink_close_threshold: 0.20,
            blink_open_threshold: 0.38,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OverridesConfig {
    pub mouth_open: Option<f32>,
    pub mouth_form: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, CameraConfig, RendererConfig, RuntimeProfile, ensure_config_file_from_example,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_partial_config_toml() {
        let config: AppConfig = toml::from_str(
            r#"
[model]
path = "public/model/custom.model3.json"

[app]
runtime_profile = "release"
window_level = "screen_saver"
window_width = 540.0
window_height = 720.0
window_capture_friendly = true

[diagnostics]
show = false

[capture.screen_capture_kit]
enabled = true
target_fps = 12
log_interval_seconds = 1.5
stalled_after_seconds = 3.0

[renderer]
disable_masks = true
high_precision_masks = true
enable_msaa = false
log_events = false
atlas_mipmaps = true
atlas_anisotropy = 8
debug_texture_mode = "uv"
hidden_drawables = ["ArtMeshDebug"]
hidden_parts = ["PartHidden"]
only_drawables = ["ArtMeshFocus"]
only_parts = ["PartFocus"]
highlight_drawables = ["ArtMeshProbe"]
highlight_parts = ["PartProbe"]

[motion]
expression = "smile"
blink_interval = 4.2

[input.mouse]
enabled = true
coordinate_space = "screen"
smoothing = 12.5
dead_zone = 0.08
invert_x = true
invert_y = true
eye_x_range = 0.8
eye_y_range = 0.7
angle_x_degrees = 24.0
angle_y_degrees = 16.0
angle_z_degrees = -8.0

[input.microphone]
enabled = true
parameter = "ParamMouthOpenY"
gain = 8.0
noise_gate = 0.05
response_curve = 0.55
smoothing = 17.0
attack = 30.0
release = 10.0
min_open = 0.02
max_open = 0.85

[input.camera]
enabled = true
device = "FaceTime HD Camera"
target_fps = 24
pose_mode = "mouse"
smoothing = 14.0
dead_zone = 0.04
invert_x = true
invert_y = true
face_x_offset = 0.10
face_y_offset = -0.08
gaze_x_offset = 0.05
gaze_y_offset = -0.04
roll_offset = 0.03
angle_x_degrees = 26.0
angle_y_degrees = 18.0
angle_z_degrees = 10.0
eye_x_range = 0.9
eye_y_range = 0.8
mouth_enabled = false
mouth_gain = 1.8
mouth_open_offset = 0.06
mouth_min_open = 0.05
mouth_max_open = 0.9
mouth_combine = "microphone"
blink_from_camera = true
blink_close_threshold = 0.22
blink_open_threshold = 0.58

[overrides]
mouth_open = 0.7
"#,
        )
        .expect("config should parse");

        assert_eq!(
            config.model.path.as_deref(),
            Some("public/model/custom.model3.json")
        );
        assert_eq!(config.app.runtime_profile, RuntimeProfile::Release);
        assert_eq!(config.app.window_level, "screen_saver");
        assert_eq!(config.app.window_width, 540.0);
        assert_eq!(config.app.window_height, 720.0);
        assert!(config.app.window_capture_friendly);
        assert!(!config.diagnostics.show);
        assert_eq!(config.capture.screen_capture_kit.enabled, Some(true));
        assert_eq!(config.capture.screen_capture_kit.target_fps, 12);
        assert_eq!(config.capture.screen_capture_kit.log_interval_seconds, 1.5);
        assert_eq!(config.capture.screen_capture_kit.stalled_after_seconds, 3.0);
        assert!(config.renderer.disable_masks);
        assert!(config.renderer.high_precision_masks);
        assert_eq!(config.renderer.enable_msaa, Some(false));
        assert_eq!(config.renderer.log_events, Some(false));
        assert!(config.renderer.atlas_mipmaps);
        assert_eq!(config.renderer.atlas_anisotropy, 8);
        assert_eq!(config.renderer.debug_texture_mode.as_deref(), Some("uv"));
        assert_eq!(config.renderer.hidden_drawables, ["ArtMeshDebug"]);
        assert_eq!(config.renderer.hidden_parts, ["PartHidden"]);
        assert_eq!(config.renderer.only_drawables, ["ArtMeshFocus"]);
        assert_eq!(config.renderer.only_parts, ["PartFocus"]);
        assert_eq!(config.renderer.highlight_drawables, ["ArtMeshProbe"]);
        assert_eq!(config.renderer.highlight_parts, ["PartProbe"]);
        assert_eq!(config.motion.expression.as_deref(), Some("smile"));
        assert_eq!(config.motion.blink_interval, 4.2);
        assert_eq!(config.motion.blink_duration, 0.18);
        assert!(config.input.mouse.enabled);
        assert_eq!(config.input.mouse.coordinate_space, "screen");
        assert_eq!(config.input.mouse.smoothing, 12.5);
        assert_eq!(config.input.mouse.dead_zone, 0.08);
        assert!(config.input.mouse.invert_x);
        assert!(config.input.mouse.invert_y);
        assert_eq!(config.input.mouse.eye_x_range, 0.8);
        assert_eq!(config.input.mouse.eye_y_range, 0.7);
        assert_eq!(config.input.mouse.angle_x_degrees, 24.0);
        assert_eq!(config.input.mouse.angle_y_degrees, 16.0);
        assert_eq!(config.input.mouse.angle_z_degrees, -8.0);
        assert!(config.input.microphone.enabled);
        assert_eq!(config.input.microphone.parameter, "ParamMouthOpenY");
        assert_eq!(config.input.microphone.gain, 8.0);
        assert_eq!(config.input.microphone.noise_gate, 0.05);
        assert_eq!(config.input.microphone.response_curve, 0.55);
        assert_eq!(config.input.microphone.smoothing, 17.0);
        assert_eq!(config.input.microphone.attack, 30.0);
        assert_eq!(config.input.microphone.release, 10.0);
        assert_eq!(config.input.microphone.min_open, 0.02);
        assert_eq!(config.input.microphone.max_open, 0.85);
        assert!(config.input.camera.enabled);
        assert_eq!(config.input.camera.device, "FaceTime HD Camera");
        assert_eq!(config.input.camera.target_fps, 24);
        assert_eq!(config.input.camera.pose_mode, "mouse");
        assert_eq!(config.input.camera.smoothing, 14.0);
        assert_eq!(config.input.camera.dead_zone, 0.04);
        assert!(config.input.camera.invert_x);
        assert!(config.input.camera.invert_y);
        assert_eq!(config.input.camera.face_x_offset, 0.10);
        assert_eq!(config.input.camera.face_y_offset, -0.08);
        assert_eq!(config.input.camera.gaze_x_offset, 0.05);
        assert_eq!(config.input.camera.gaze_y_offset, -0.04);
        assert_eq!(config.input.camera.roll_offset, 0.03);
        assert_eq!(config.input.camera.angle_x_degrees, 26.0);
        assert_eq!(config.input.camera.angle_y_degrees, 18.0);
        assert_eq!(config.input.camera.angle_z_degrees, 10.0);
        assert_eq!(config.input.camera.eye_x_range, 0.9);
        assert_eq!(config.input.camera.eye_y_range, 0.8);
        assert!(!config.input.camera.mouth_enabled);
        assert_eq!(config.input.camera.mouth_gain, 1.8);
        assert_eq!(config.input.camera.mouth_open_offset, 0.06);
        assert_eq!(config.input.camera.mouth_min_open, 0.05);
        assert_eq!(config.input.camera.mouth_max_open, 0.9);
        assert_eq!(config.input.camera.mouth_combine, "microphone");
        assert!(config.input.camera.blink_from_camera);
        assert_eq!(config.input.camera.blink_close_threshold, 0.22);
        assert_eq!(config.input.camera.blink_open_threshold, 0.58);
        assert_eq!(config.overrides.mouth_open, Some(0.7));
        assert_eq!(config.overrides.mouth_form, None);
    }

    #[test]
    fn resolves_model_path_precedence() {
        let default_config = AppConfig::default();
        assert_eq!(
            default_config.resolved_model_path(None),
            "public/model/0.model3.json"
        );

        let config_model: AppConfig = toml::from_str(
            r#"
[model]
path = "public/model/from-config.model3.json"
"#,
        )
        .expect("config should parse");
        assert_eq!(
            config_model.resolved_model_path(None),
            "public/model/from-config.model3.json"
        );
        assert_eq!(
            config_model.resolved_model_path(Some("public/model/from-cli.model3.json")),
            "public/model/from-cli.model3.json"
        );
    }

    #[test]
    fn creates_missing_local_config_from_example() {
        let root = unique_temp_dir("config-create");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let config_path = root.join("vtube-studio-rs.dev.toml");
        let example_path = root.join("vtube-studio-rs.dev.example.toml");
        fs::write(
            &example_path,
            "[model]\npath = \"public/model/from-example.model3.json\"\n",
        )
        .expect("example config should be written");

        assert!(
            ensure_config_file_from_example(&config_path, &example_path)
                .expect("config should be created")
        );
        assert_eq!(
            fs::read_to_string(&config_path).expect("created config should be readable"),
            fs::read_to_string(&example_path).expect("example config should be readable")
        );
        assert!(
            !ensure_config_file_from_example(&config_path, &example_path)
                .expect("existing config should be preserved")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_example_keeps_default_config_path_optional() {
        let root = unique_temp_dir("config-missing-example");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let config_path = root.join("vtube-studio-rs.dev.toml");
        let example_path = root.join("missing.example.toml");

        assert!(
            !ensure_config_file_from_example(&config_path, &example_path)
                .expect("missing example should not be fatal")
        );
        assert!(!config_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vtube-studio-rs-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn camera_defaults_use_mirrored_horizontal_pose() {
        let config = CameraConfig::default();

        assert!(config.invert_x);
        assert!(!config.invert_y);
    }

    #[test]
    fn screen_capture_kit_enabled_defaults_follow_runtime_profile() {
        let config = super::ScreenCaptureKitConfig::default();

        assert!(config.enabled(RuntimeProfile::Development));
        assert!(!config.enabled(RuntimeProfile::Release));
    }

    #[test]
    fn output_mode_accepts_window_and_internal_aliases() {
        assert_eq!(
            super::OutputMode::from_config("window"),
            super::OutputMode::Window
        );
        assert_eq!(
            super::OutputMode::from_config("syphon"),
            super::OutputMode::Window
        );
        assert_eq!(
            super::OutputMode::from_config("internal"),
            super::OutputMode::Internal
        );
        assert_eq!(
            super::OutputMode::from_config("obs_internal"),
            super::OutputMode::Internal
        );
        assert!(super::OutputMode::Window.uses_window());
        assert!(!super::OutputMode::Internal.uses_window());
        assert_eq!(
            super::InternalOutputProducer::from_config("none"),
            super::InternalOutputProducer::None
        );
        assert_eq!(
            super::InternalOutputProducer::from_config("io_surface"),
            super::InternalOutputProducer::Iosurface
        );
        let config = super::InternalOutputConfig::default();
        assert_eq!(
            config.manifest_path(),
            "target/internal-output/iosurface.json"
        );
    }

    #[test]
    fn renderer_runtime_profile_defaults_can_be_overridden() {
        let renderer = RendererConfig::default();
        assert!(renderer.enable_msaa(RuntimeProfile::Development));
        assert!(renderer.log_events(RuntimeProfile::Development));
        assert!(!renderer.enable_msaa(RuntimeProfile::Release));
        assert!(!renderer.log_events(RuntimeProfile::Release));

        let renderer: RendererConfig = toml::from_str(
            r#"
enable_msaa = true
log_events = true
"#,
        )
        .expect("renderer config should parse");
        assert!(renderer.enable_msaa(RuntimeProfile::Release));
        assert!(renderer.log_events(RuntimeProfile::Release));
    }
}
