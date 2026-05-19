use serde::Deserialize;
use std::fs;
use std::path::Path;

const DEFAULT_CONFIG_PATH: &str = "vtube-studio-rs.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub diagnostics: DiagnosticsConfig,
    pub renderer: RendererConfig,
    pub motion: MotionConfig,
    pub input: InputConfig,
    pub overrides: OverridesConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self, String> {
        let path = Path::new(DEFAULT_CONFIG_PATH);
        if !path.is_file() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let config = toml::from_str(&text)
            .map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;
        println!("Loaded config: {}", path.display());
        Ok(config)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            diagnostics: DiagnosticsConfig::default(),
            renderer: RendererConfig::default(),
            motion: MotionConfig::default(),
            input: InputConfig::default(),
            overrides: OverridesConfig::default(),
        }
    }
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
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            disable_masks: false,
        }
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
    use super::AppConfig;

    #[test]
    fn parses_partial_config_toml() {
        let config: AppConfig = toml::from_str(
            r#"
[diagnostics]
show = false

[renderer]
disable_masks = true

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

        assert!(!config.diagnostics.show);
        assert!(config.renderer.disable_masks);
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
}
