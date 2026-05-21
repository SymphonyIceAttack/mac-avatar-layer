use serde::Deserialize;
use std::fs;
use std::path::Path;

const DEVELOPMENT_CONFIG_PATH: &str = "vtube-studio-rs.dev.toml";
const BUILD_CONFIG_PATH: &str = "vtube-studio-rs.build.toml";
const DEFAULT_MODEL_PATH: &str = "public/model/0.model3.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub app: AppRuntimeConfig,
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
        if path.is_file() {
            return Self::load_from_path(path);
        }

        let mut config = Self::default();
        config.app.runtime_profile = default_runtime_profile();
        Ok(config)
    }

    fn load_from_path(path: &Path) -> Result<Self, String> {
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

fn default_config_path() -> &'static str {
    if cfg!(debug_assertions) {
        DEVELOPMENT_CONFIG_PATH
    } else {
        BUILD_CONFIG_PATH
    }
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

    pub fn default_log_renderer_events(self) -> bool {
        matches!(self, Self::Development)
    }
}

impl Default for RuntimeProfile {
    fn default() -> Self {
        Self::Development
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AppRuntimeConfig {
    pub runtime_profile: RuntimeProfile,
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MouseConfig {
    pub enabled: bool,
    pub smoothing: f32,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            smoothing: 10.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MicrophoneConfig {
    pub enabled: bool,
    pub gain: f32,
    pub noise_gate: f32,
    pub smoothing: f32,
}

impl Default for MicrophoneConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gain: 7.0,
            noise_gate: 0.025,
            smoothing: 18.0,
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
    use super::{AppConfig, RendererConfig, RuntimeProfile};

    #[test]
    fn parses_partial_config_toml() {
        let config: AppConfig = toml::from_str(
            r#"
[model]
path = "public/model/custom.model3.json"

[app]
runtime_profile = "release"

[diagnostics]
show = false

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
smoothing = 12.5

[input.microphone]
enabled = true
gain = 8.0
noise_gate = 0.05

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
        assert!(!config.diagnostics.show);
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
        assert_eq!(config.input.mouse.smoothing, 12.5);
        assert!(config.input.microphone.enabled);
        assert_eq!(config.input.microphone.gain, 8.0);
        assert_eq!(config.input.microphone.noise_gate, 0.05);
        assert_eq!(config.input.microphone.smoothing, 18.0);
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
