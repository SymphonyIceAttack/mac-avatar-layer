use crate::cubism::CubismModelRuntime;
use crate::live2d_model::Live2dModel;
use std::f32::consts::TAU;
use std::time::Duration;

pub struct MotionController {
    eye_blink_ids: Vec<String>,
    elapsed: f32,
    blink_elapsed: f32,
    blink_interval: f32,
    blink_duration: f32,
    mouth_open_override: Option<f32>,
    mouth_form_override: Option<f32>,
}

impl MotionController {
    pub fn new(model: &Live2dModel) -> Self {
        let eye_blink_ids = model
            .groups
            .iter()
            .find(|group| group.target == "Parameter" && group.name == "EyeBlink")
            .map(|group| group.ids.clone())
            .filter(|ids| !ids.is_empty())
            .unwrap_or_else(|| vec!["ParamEyeLOpen".to_string(), "ParamEyeROpen".to_string()]);

        Self {
            eye_blink_ids,
            elapsed: 0.0,
            blink_elapsed: 0.0,
            blink_interval: env_f32("VTUBE_RS_BLINK_INTERVAL").unwrap_or(3.8).max(0.5),
            blink_duration: env_f32("VTUBE_RS_BLINK_DURATION").unwrap_or(0.18).max(0.05),
            mouth_open_override: env_f32("VTUBE_RS_MOUTH_OPEN"),
            mouth_form_override: env_f32("VTUBE_RS_MOUTH_FORM"),
        }
    }

    pub fn apply(&mut self, runtime: &mut CubismModelRuntime, delta: Duration) {
        let delta = delta.as_secs_f32().clamp(0.0, 0.1);
        self.elapsed += delta;
        self.blink_elapsed += delta;

        let breath = (self.elapsed * 0.65 * TAU).sin() * 0.5 + 0.5;
        runtime.set_parameter_value("ParamBreath", breath);

        let eye_open = self.eye_open_value();
        for id in &self.eye_blink_ids {
            runtime.set_parameter_value(id, eye_open);
        }

        if let Some(value) = self.mouth_open_override {
            runtime.set_parameter_value("ParamMouthOpenY", value);
        }
        if let Some(value) = self.mouth_form_override {
            runtime.set_parameter_value("ParamMouthForm", value);
        }

        runtime.update();
    }

    fn eye_open_value(&mut self) -> f32 {
        if self.blink_elapsed >= self.blink_interval + self.blink_duration {
            self.blink_elapsed = 0.0;
        }

        if self.blink_elapsed < self.blink_interval {
            return 1.0;
        }

        let t = ((self.blink_elapsed - self.blink_interval) / self.blink_duration).clamp(0.0, 1.0);
        if t < 0.5 {
            1.0 - ease_in_out(t * 2.0)
        } else {
            ease_in_out((t - 0.5) * 2.0)
        }
    }
}

fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name).ok()?.parse().ok()
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
