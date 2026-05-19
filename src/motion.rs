use crate::config::{AppConfig, MicrophoneConfig, MouseConfig};
use crate::cubism::CubismModelRuntime;
use crate::live2d_model::Live2dModel;
use serde::Deserialize;
use std::f32::consts::TAU;
use std::fs;
use std::path::Path;
use std::time::Duration;

pub struct MotionController {
    eye_blink_ids: Vec<String>,
    idle_motion: Option<MotionPlayer>,
    expression: Option<ExpressionRuntime>,
    mouse_driver: Option<MouseDriver>,
    mic_driver: Option<MicMouthDriver>,
    physics: Option<PhysicsRig>,
    elapsed: f32,
    blink_elapsed: f32,
    blink_interval: f32,
    blink_duration: f32,
    mouth_open_override: Option<f32>,
    mouth_form_override: Option<f32>,
}

#[derive(Default)]
pub struct MotionInput {
    pub pointer: Option<[f32; 2]>,
    pub mouth_level: Option<f32>,
}

impl MotionController {
    pub fn new(model: &Live2dModel, config: &AppConfig) -> Self {
        let eye_blink_ids = model
            .groups
            .iter()
            .find(|group| group.target == "Parameter" && group.name == "EyeBlink")
            .map(|group| group.ids.clone())
            .filter(|ids| !ids.is_empty())
            .unwrap_or_else(|| vec!["ParamEyeLOpen".to_string(), "ParamEyeROpen".to_string()]);
        let physics = model
            .physics
            .as_deref()
            .and_then(|path| match PhysicsRig::load(path) {
                Ok(physics) => {
                    println!(
                        "Loaded lightweight physics: {} settings, {} outputs",
                        physics.settings.len(),
                        physics.output_count()
                    );
                    Some(physics)
                }
                Err(error) => {
                    eprintln!("Failed to load lightweight physics: {error}");
                    None
                }
            });
        let idle_motion = load_idle_motion(model);
        let expression = load_expression(model, config.motion.expression.as_deref());
        let mouse_driver = MouseDriver::from_config(&config.input.mouse);
        let mic_driver = MicMouthDriver::from_config(&config.input.microphone);

        Self {
            eye_blink_ids,
            idle_motion,
            expression,
            mouse_driver,
            mic_driver,
            physics,
            elapsed: 0.0,
            blink_elapsed: 0.0,
            blink_interval: config.motion.blink_interval.max(0.5),
            blink_duration: config.motion.blink_duration.max(0.05),
            mouth_open_override: config.overrides.mouth_open,
            mouth_form_override: config.overrides.mouth_form,
        }
    }

    pub fn apply(
        &mut self,
        runtime: &mut CubismModelRuntime,
        delta: Duration,
        input: &MotionInput,
    ) {
        let delta = delta.as_secs_f32().clamp(0.0, 0.1);
        self.elapsed += delta;
        self.blink_elapsed += delta;

        let breath = (self.elapsed * 0.65 * TAU).sin() * 0.5 + 0.5;
        runtime.set_parameter_value("ParamBreath", breath);

        let eye_open = self.eye_open_value();
        for id in &self.eye_blink_ids {
            runtime.set_parameter_value(id, eye_open);
        }

        if let Some(motion) = &mut self.idle_motion {
            motion.apply(runtime, delta);
        }

        if let Some(expression) = &self.expression {
            expression.apply(runtime);
        }

        if let Some(mouse_driver) = &mut self.mouse_driver {
            mouse_driver.apply(runtime, input.pointer, delta);
        }

        if let Some(mic_driver) = &mut self.mic_driver {
            mic_driver.apply(runtime, input.mouth_level, delta);
        }

        if let Some(physics) = &mut self.physics {
            physics.apply(runtime, delta);
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

fn load_idle_motion(model: &Live2dModel) -> Option<MotionPlayer> {
    let motion = model
        .motions
        .get("Idle")
        .or_else(|| model.motions.get("idle"))
        .and_then(|motions| motions.first())?;

    match MotionClip::load(&motion.file) {
        Ok(clip) => {
            println!(
                "Loaded idle motion: {} duration {:.3}s curves {}",
                motion.file.display(),
                clip.duration,
                clip.curves.len()
            );
            Some(MotionPlayer::new(
                clip,
                motion.fade_in_time.unwrap_or(0.0),
                motion.fade_out_time.unwrap_or(0.0),
            ))
        }
        Err(error) => {
            eprintln!(
                "Failed to load idle motion {}: {error}",
                motion.file.display()
            );
            None
        }
    }
}

fn load_expression(model: &Live2dModel, requested: Option<&str>) -> Option<ExpressionRuntime> {
    let requested = requested?;
    let expression = model
        .expressions
        .iter()
        .find(|expression| expression.name == requested)
        .or_else(|| {
            requested
                .parse::<usize>()
                .ok()
                .and_then(|index| model.expressions.get(index))
        });
    let Some(expression) = expression else {
        eprintln!("Expression '{requested}' was not found in model3.json");
        return None;
    };

    match ExpressionRuntime::load(&expression.file) {
        Ok(runtime) => {
            println!(
                "Loaded expression {}: {} parameters",
                expression.name,
                runtime.parameters.len()
            );
            Some(runtime)
        }
        Err(error) => {
            eprintln!(
                "Failed to load expression {} from {}: {error}",
                expression.name,
                expression.file.display()
            );
            None
        }
    }
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct PhysicsRig {
    settings: Vec<PhysicsSetting>,
    gravity: Vec2,
    wind: Vec2,
    fps: f32,
    remaining_time: f32,
    initialized: bool,
}

struct MouseDriver {
    eye_x: f32,
    eye_y: f32,
    angle_x: f32,
    angle_y: f32,
    angle_z: f32,
    smoothing: f32,
}

impl MouseDriver {
    fn from_config(config: &MouseConfig) -> Option<Self> {
        config.enabled.then(|| {
            println!("Mouse tracking enabled: driving eye ball and head angle parameters");
            Self {
                eye_x: 0.0,
                eye_y: 0.0,
                angle_x: 0.0,
                angle_y: 0.0,
                angle_z: 0.0,
                smoothing: config.smoothing.clamp(1.0, 60.0),
            }
        })
    }

    fn apply(&mut self, runtime: &mut CubismModelRuntime, pointer: Option<[f32; 2]>, delta: f32) {
        let Some([x, y]) = pointer else {
            return;
        };
        let x = x.clamp(-1.0, 1.0);
        let y = y.clamp(-1.0, 1.0);
        let alpha = (1.0 - (-self.smoothing * delta).exp()).clamp(0.0, 1.0);

        self.eye_x = lerp(self.eye_x, x, alpha);
        self.eye_y = lerp(self.eye_y, y, alpha);
        self.angle_x = lerp(self.angle_x, x * 30.0, alpha);
        self.angle_y = lerp(self.angle_y, y * 22.0, alpha);
        self.angle_z = lerp(self.angle_z, -x * 12.0, alpha);

        runtime.set_parameter_value("ParamEyeBallX", self.eye_x);
        runtime.set_parameter_value("ParamEyeBallY", self.eye_y);
        runtime.set_parameter_value("ParamAngleX", self.angle_x);
        runtime.set_parameter_value("ParamAngleY", self.angle_y);
        runtime.set_parameter_value("ParamAngleZ", self.angle_z);
    }
}

struct MicMouthDriver {
    value: f32,
    gain: f32,
    noise_gate: f32,
    smoothing: f32,
}

impl MicMouthDriver {
    fn from_config(config: &MicrophoneConfig) -> Option<Self> {
        config.enabled.then(|| Self {
            value: 0.0,
            gain: config.gain.clamp(0.1, 80.0),
            noise_gate: config.noise_gate.clamp(0.0, 0.5),
            smoothing: config.smoothing.clamp(1.0, 80.0),
        })
    }

    fn apply(&mut self, runtime: &mut CubismModelRuntime, level: Option<f32>, delta: f32) {
        let Some(level) = level else {
            return;
        };
        let level = level.clamp(0.0, 1.0);
        let target = if level <= self.noise_gate {
            0.0
        } else {
            ((level - self.noise_gate) / (1.0 - self.noise_gate)).clamp(0.0, 1.0) * self.gain
        }
        .clamp(0.0, 1.0);
        let alpha = (1.0 - (-self.smoothing * delta).exp()).clamp(0.0, 1.0);
        self.value = lerp(self.value, target, alpha);
        runtime.set_parameter_value("ParamMouthOpenY", self.value);
    }
}

fn lerp(current: f32, target: f32, alpha: f32) -> f32 {
    current + (target - current) * alpha
}

struct MotionPlayer {
    clip: MotionClip,
    elapsed: f32,
    fade_in_time: f32,
    _fade_out_time: f32,
}

impl MotionPlayer {
    fn new(clip: MotionClip, fade_in_time: f32, fade_out_time: f32) -> Self {
        Self {
            clip,
            elapsed: 0.0,
            fade_in_time,
            _fade_out_time: fade_out_time,
        }
    }

    fn apply(&mut self, runtime: &mut CubismModelRuntime, delta: f32) {
        if self.clip.duration <= f32::EPSILON {
            return;
        }

        self.elapsed += delta;
        let time = if self.clip.looping {
            self.elapsed % self.clip.duration
        } else {
            self.elapsed.min(self.clip.duration)
        };
        let fade_weight = if self.fade_in_time > f32::EPSILON {
            (self.elapsed / self.fade_in_time).clamp(0.0, 1.0)
        } else {
            1.0
        };

        for curve in &self.clip.curves {
            if curve.target != "Parameter" {
                continue;
            }
            let value = curve.evaluate(time);
            if fade_weight >= 1.0 {
                runtime.set_parameter_value(&curve.id, value);
            } else if let Some(parameter) = runtime.parameter(&curve.id) {
                let blended = parameter.value * (1.0 - fade_weight) + value * fade_weight;
                runtime.set_parameter_value(&curve.id, blended);
            }
        }
    }
}

struct MotionClip {
    duration: f32,
    looping: bool,
    curves: Vec<MotionCurve>,
}

impl MotionClip {
    fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let manifest: MotionManifest = serde_json::from_str(&text)
            .map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;
        let curves = manifest
            .curves
            .into_iter()
            .filter_map(MotionCurve::from_manifest)
            .collect();

        Ok(Self {
            duration: manifest.meta.duration,
            looping: manifest.meta.looping,
            curves,
        })
    }
}

struct MotionCurve {
    target: String,
    id: String,
    segments: Vec<MotionSegment>,
}

impl MotionCurve {
    fn from_manifest(manifest: MotionCurveManifest) -> Option<Self> {
        Some(Self {
            target: manifest.target,
            id: manifest.id,
            segments: parse_motion_segments(&manifest.segments)?,
        })
    }

    fn evaluate(&self, time: f32) -> f32 {
        if self.segments.is_empty() {
            return 0.0;
        }

        for segment in &self.segments {
            if time <= segment.end_time() {
                return segment.evaluate(time);
            }
        }

        self.segments
            .last()
            .map(MotionSegment::end_value)
            .unwrap_or(0.0)
    }
}

enum MotionSegment {
    Linear {
        start_time: f32,
        start_value: f32,
        end_time: f32,
        end_value: f32,
    },
    Bezier {
        start_time: f32,
        start_value: f32,
        control1_time: f32,
        control1_value: f32,
        control2_time: f32,
        control2_value: f32,
        end_time: f32,
        end_value: f32,
    },
    Stepped {
        start_value: f32,
        end_time: f32,
        end_value: f32,
    },
    InverseStepped {
        end_time: f32,
        end_value: f32,
    },
}

impl MotionSegment {
    fn end_time(&self) -> f32 {
        match self {
            Self::Linear { end_time, .. }
            | Self::Bezier { end_time, .. }
            | Self::Stepped { end_time, .. }
            | Self::InverseStepped { end_time, .. } => *end_time,
        }
    }

    fn end_value(&self) -> f32 {
        match self {
            Self::Linear { end_value, .. }
            | Self::Bezier { end_value, .. }
            | Self::Stepped { end_value, .. }
            | Self::InverseStepped { end_value, .. } => *end_value,
        }
    }

    fn evaluate(&self, time: f32) -> f32 {
        match *self {
            Self::Linear {
                start_time,
                start_value,
                end_time,
                end_value,
            } => {
                let t = normalized_time(time, start_time, end_time);
                start_value + (end_value - start_value) * t
            }
            Self::Bezier {
                start_time,
                start_value,
                control1_time,
                control1_value,
                control2_time,
                control2_value,
                end_time,
                end_value,
            } => evaluate_bezier_by_time(
                time,
                [
                    (start_time, start_value),
                    (control1_time, control1_value),
                    (control2_time, control2_value),
                    (end_time, end_value),
                ],
            ),
            Self::Stepped { start_value, .. } => start_value,
            Self::InverseStepped { end_value, .. } => end_value,
        }
    }
}

fn parse_motion_segments(values: &[f32]) -> Option<Vec<MotionSegment>> {
    if values.len() < 2 {
        return None;
    }

    let mut segments = Vec::new();
    let mut index = 2;
    let mut start_time = values[0];
    let mut start_value = values[1];

    while index < values.len() {
        let segment_type = values[index] as i32;
        index += 1;
        match segment_type {
            0 => {
                let end_time = *values.get(index)?;
                let end_value = *values.get(index + 1)?;
                index += 2;
                segments.push(MotionSegment::Linear {
                    start_time,
                    start_value,
                    end_time,
                    end_value,
                });
                start_time = end_time;
                start_value = end_value;
            }
            1 => {
                let control1_time = *values.get(index)?;
                let control1_value = *values.get(index + 1)?;
                let control2_time = *values.get(index + 2)?;
                let control2_value = *values.get(index + 3)?;
                let end_time = *values.get(index + 4)?;
                let end_value = *values.get(index + 5)?;
                index += 6;
                segments.push(MotionSegment::Bezier {
                    start_time,
                    start_value,
                    control1_time,
                    control1_value,
                    control2_time,
                    control2_value,
                    end_time,
                    end_value,
                });
                start_time = end_time;
                start_value = end_value;
            }
            2 => {
                let end_time = *values.get(index)?;
                let end_value = *values.get(index + 1)?;
                index += 2;
                segments.push(MotionSegment::Stepped {
                    start_value,
                    end_time,
                    end_value,
                });
                start_time = end_time;
                start_value = end_value;
            }
            3 => {
                let end_time = *values.get(index)?;
                let end_value = *values.get(index + 1)?;
                index += 2;
                segments.push(MotionSegment::InverseStepped {
                    end_time,
                    end_value,
                });
                start_time = end_time;
                start_value = end_value;
            }
            _ => return None,
        }
    }

    Some(segments)
}

fn normalized_time(time: f32, start: f32, end: f32) -> f32 {
    if (end - start).abs() <= f32::EPSILON {
        1.0
    } else {
        ((time - start) / (end - start)).clamp(0.0, 1.0)
    }
}

fn evaluate_bezier_by_time(time: f32, points: [(f32, f32); 4]) -> f32 {
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..12 {
        let mid = (low + high) * 0.5;
        if cubic_bezier(mid, points[0].0, points[1].0, points[2].0, points[3].0) < time {
            low = mid;
        } else {
            high = mid;
        }
    }
    let t = (low + high) * 0.5;
    cubic_bezier(t, points[0].1, points[1].1, points[2].1, points[3].1)
}

fn cubic_bezier(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let inv = 1.0 - t;
    inv * inv * inv * p0 + 3.0 * inv * inv * t * p1 + 3.0 * inv * t * t * p2 + t * t * t * p3
}

struct ExpressionRuntime {
    parameters: Vec<ExpressionParameter>,
}

impl ExpressionRuntime {
    fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let manifest: ExpressionManifest = serde_json::from_str(&text)
            .map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;
        Ok(Self {
            parameters: manifest
                .parameters
                .into_iter()
                .map(|parameter| ExpressionParameter {
                    id: parameter.id,
                    value: parameter.value,
                    blend: parameter.blend.unwrap_or_default(),
                })
                .collect(),
        })
    }

    fn apply(&self, runtime: &mut CubismModelRuntime) {
        for parameter in &self.parameters {
            let Some(current) = runtime.parameter(&parameter.id) else {
                continue;
            };
            let value = match parameter.blend {
                ExpressionBlend::Add => current.value + parameter.value,
                ExpressionBlend::Multiply => current.value * parameter.value,
                ExpressionBlend::Overwrite => parameter.value,
            };
            runtime.set_parameter_value(&parameter.id, value);
        }
    }
}

struct ExpressionParameter {
    id: String,
    value: f32,
    blend: ExpressionBlend,
}

struct PhysicsSetting {
    inputs: Vec<PhysicsInput>,
    outputs: Vec<PhysicsOutput>,
    particles: Vec<PhysicsParticle>,
    previous_outputs: Vec<f32>,
    current_outputs: Vec<f32>,
    normalization_position: NormalizationRange,
    normalization_angle: NormalizationRange,
}

struct PhysicsInput {
    source_id: String,
    kind: PhysicsValueKind,
    weight: f32,
    reflect: bool,
    cached_value: Option<f32>,
}

struct PhysicsOutput {
    destination_id: String,
    kind: PhysicsValueKind,
    scale: f32,
    weight: f32,
    reflect: bool,
    vertex_index: usize,
}

#[derive(Clone, Copy)]
enum PhysicsValueKind {
    X,
    Y,
    Angle,
}

#[derive(Clone, Copy, Debug)]
struct NormalizationRange {
    min: f32,
    default: f32,
    max: f32,
}

impl Default for NormalizationRange {
    fn default() -> Self {
        Self {
            min: -1.0,
            default: 0.0,
            max: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    const ZERO: Self = Self { x: 0.0, y: 0.0 };
    const DOWN: Self = Self { x: 0.0, y: -1.0 };

    fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    fn normalized(self) -> Self {
        let length = self.length();
        if length <= f32::EPSILON {
            Self::ZERO
        } else {
            self / length
        }
    }

    fn rotate(self, radians: f32) -> Self {
        let sin = radians.sin();
        let cos = radians.cos();
        Self {
            x: cos * self.x - sin * self.y,
            y: sin * self.x + cos * self.y,
        }
    }
}

impl std::ops::Add for Vec2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl std::ops::MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl std::ops::Div<f32> for Vec2 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

#[derive(Clone, Debug)]
struct PhysicsParticle {
    position: Vec2,
    last_position: Vec2,
    last_gravity: Vec2,
    velocity: Vec2,
    force: Vec2,
    mobility: f32,
    delay: f32,
    acceleration: f32,
    radius: f32,
}

impl PhysicsRig {
    fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let manifest: PhysicsManifest = serde_json::from_str(&text)
            .map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;

        let settings = manifest
            .physics_settings
            .into_iter()
            .map(|setting| {
                let inputs = setting
                    .input
                    .into_iter()
                    .filter(|input| input.source.target == "Parameter")
                    .map(|input| PhysicsInput {
                        source_id: input.source.id,
                        kind: input.kind.into(),
                        weight: input.weight / 100.0,
                        reflect: input.reflect,
                        cached_value: None,
                    })
                    .collect();
                let outputs: Vec<_> = setting
                    .output
                    .into_iter()
                    .filter(|output| output.destination.target == "Parameter")
                    .map(|output| PhysicsOutput {
                        destination_id: output.destination.id,
                        kind: output.kind.into(),
                        scale: output.scale,
                        weight: output.weight / 100.0,
                        reflect: output.reflect,
                        vertex_index: output.vertex_index,
                    })
                    .collect();
                let output_count = outputs.len();

                PhysicsSetting {
                    inputs,
                    outputs,
                    particles: initialize_particles(setting.vertices),
                    previous_outputs: vec![0.0; output_count],
                    current_outputs: vec![0.0; output_count],
                    normalization_position: setting
                        .normalization
                        .as_ref()
                        .map(|normalization| normalization.position.into())
                        .unwrap_or_default(),
                    normalization_angle: setting
                        .normalization
                        .as_ref()
                        .map(|normalization| normalization.angle.into())
                        .unwrap_or_default(),
                }
            })
            .collect();

        let gravity = manifest
            .meta
            .as_ref()
            .and_then(|meta| meta.effective_forces.as_ref())
            .map(|forces| forces.gravity.into())
            .unwrap_or(Vec2::DOWN);
        let wind = manifest
            .meta
            .as_ref()
            .and_then(|meta| meta.effective_forces.as_ref())
            .map(|forces| forces.wind.into())
            .unwrap_or(Vec2::ZERO);
        let fps = manifest
            .meta
            .as_ref()
            .and_then(|meta| meta.fps)
            .unwrap_or(0.0);

        Ok(Self {
            settings,
            gravity,
            wind,
            fps,
            remaining_time: 0.0,
            initialized: false,
        })
    }

    fn output_count(&self) -> usize {
        self.settings
            .iter()
            .map(|setting| setting.outputs.len())
            .sum()
    }

    fn apply(&mut self, runtime: &mut CubismModelRuntime, delta: f32) {
        if delta <= 0.0 {
            return;
        }

        if !self.initialized {
            self.stabilize(runtime);
            self.initialized = true;
        }

        const MAX_DELTA_TIME: f32 = 5.0;
        self.remaining_time += delta;
        if self.remaining_time > MAX_DELTA_TIME {
            self.remaining_time = 0.0;
        }

        let physics_delta = if self.fps > 0.0 {
            (1.0 / self.fps).clamp(0.001, 0.1)
        } else {
            delta
        };

        while self.remaining_time >= physics_delta {
            for setting in &mut self.settings {
                setting
                    .previous_outputs
                    .copy_from_slice(&setting.current_outputs);
            }

            let input_weight = (physics_delta / self.remaining_time).clamp(0.0, 1.0);
            self.step(runtime, physics_delta, input_weight);
            self.remaining_time -= physics_delta;
        }

        let alpha = if physics_delta > f32::EPSILON {
            (self.remaining_time / physics_delta).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.interpolate_outputs(runtime, alpha);
    }

    fn stabilize(&mut self, runtime: &mut CubismModelRuntime) {
        for setting in &mut self.settings {
            let (total_translation, total_angle) = setting.sample_inputs(runtime, 1.0, false);
            if setting.particles.len() < 2 {
                continue;
            }

            update_particles_for_stabilization(
                &mut setting.particles,
                total_translation.rotate(-total_angle.to_radians()),
                total_angle,
                self.wind,
                0.001 * setting.normalization_position.max.abs(),
            );

            calculate_outputs(setting, self.gravity);
            setting
                .previous_outputs
                .copy_from_slice(&setting.current_outputs);
            apply_outputs(runtime, setting, &setting.current_outputs);
        }
    }

    fn step(&mut self, runtime: &mut CubismModelRuntime, delta: f32, input_weight: f32) {
        for setting in &mut self.settings {
            let (total_translation, total_angle) =
                setting.sample_inputs(runtime, input_weight, true);
            if setting.particles.len() < 2 {
                continue;
            }

            update_particles(
                &mut setting.particles,
                total_translation.rotate(-total_angle.to_radians()),
                total_angle,
                self.wind,
                0.001 * setting.normalization_position.max.abs(),
                delta,
            );

            calculate_outputs(setting, self.gravity);
            apply_outputs(runtime, setting, &setting.current_outputs);
        }
    }

    fn interpolate_outputs(&self, runtime: &mut CubismModelRuntime, alpha: f32) {
        for setting in &self.settings {
            let mut outputs = Vec::with_capacity(setting.outputs.len());
            for index in 0..setting.outputs.len() {
                outputs.push(
                    setting.previous_outputs[index] * (1.0 - alpha)
                        + setting.current_outputs[index] * alpha,
                );
            }
            apply_outputs(runtime, setting, &outputs);
        }
    }
}

impl PhysicsSetting {
    fn sample_inputs(
        &mut self,
        runtime: &CubismModelRuntime,
        input_weight: f32,
        update_cache: bool,
    ) -> (Vec2, f32) {
        let mut total_translation = Vec2::ZERO;
        let mut total_angle = 0.0;

        for input in &mut self.inputs {
            let Some(parameter) = runtime.parameter(&input.source_id) else {
                continue;
            };
            let value = if update_cache {
                let previous = input.cached_value.unwrap_or(parameter.value);
                let value = previous * (1.0 - input_weight) + parameter.value * input_weight;
                input.cached_value = Some(value);
                value
            } else {
                input.cached_value = Some(parameter.value);
                parameter.value
            };

            let normalized = normalize_parameter(
                value,
                parameter.min,
                parameter.max,
                parameter.default,
                match input.kind {
                    PhysicsValueKind::Angle => self.normalization_angle,
                    PhysicsValueKind::X | PhysicsValueKind::Y => self.normalization_position,
                },
                input.reflect,
            );

            match input.kind {
                PhysicsValueKind::X => total_translation.x += normalized * input.weight,
                PhysicsValueKind::Y => total_translation.y += normalized * input.weight,
                PhysicsValueKind::Angle => total_angle += normalized * input.weight,
            }
        }

        (total_translation, total_angle)
    }
}

fn calculate_outputs(setting: &mut PhysicsSetting, gravity: Vec2) {
    for (output_index, output) in setting.outputs.iter().enumerate() {
        let particle_index = output.vertex_index;
        if particle_index < 1 || particle_index >= setting.particles.len() {
            continue;
        }

        let translation = setting.particles[particle_index].position
            - setting.particles[particle_index - 1].position;
        setting.current_outputs[output_index] = output_value(
            output.kind,
            translation,
            &setting.particles,
            particle_index,
            output.reflect,
            gravity,
        );
    }
}

fn apply_outputs(runtime: &mut CubismModelRuntime, setting: &PhysicsSetting, values: &[f32]) {
    for (output, value) in setting.outputs.iter().zip(values) {
        apply_output_parameter(runtime, &output.destination_id, *value, output);
    }
}

fn initialize_particles(vertices: Vec<PhysicsVertexManifest>) -> Vec<PhysicsParticle> {
    let mut particles: Vec<PhysicsParticle> = Vec::with_capacity(vertices.len());
    for (index, vertex) in vertices.into_iter().enumerate() {
        let mut position = Vec2::from(vertex.position);
        if index > 0 && vertex.radius > 0.0 {
            let previous = particles[index - 1].position;
            let direction = (position - previous).normalized();
            position = previous
                + if direction.length() <= f32::EPSILON {
                    Vec2::new(0.0, vertex.radius)
                } else {
                    direction * vertex.radius
                };
        }

        particles.push(PhysicsParticle {
            position,
            last_position: position,
            last_gravity: Vec2::DOWN,
            velocity: Vec2::ZERO,
            force: Vec2::ZERO,
            mobility: vertex.mobility,
            delay: vertex.delay,
            acceleration: vertex.acceleration,
            radius: vertex.radius,
        });
    }

    particles
}

fn normalize_parameter(
    value: f32,
    min: f32,
    max: f32,
    _default: f32,
    normalization: NormalizationRange,
    inverted: bool,
) -> f32 {
    let max_value = max.max(min);
    let min_value = max.min(min);
    let value = value.clamp(min_value, max_value);
    let min_norm = normalization.min.min(normalization.max);
    let max_norm = normalization.min.max(normalization.max);
    let middle_norm = normalization.default;
    let middle_value = min_value + ((max_value - min_value) / 2.0);
    let param_value = value - middle_value;

    let result = if param_value > 0.0 {
        let normalized_length = max_norm - middle_norm;
        let parameter_length = max_value - middle_value;
        if parameter_length.abs() <= f32::EPSILON {
            0.0
        } else {
            param_value * (normalized_length / parameter_length) + middle_norm
        }
    } else if param_value < 0.0 {
        let normalized_length = min_norm - middle_norm;
        let parameter_length = min_value - middle_value;
        if parameter_length.abs() <= f32::EPSILON {
            0.0
        } else {
            param_value * (normalized_length / parameter_length) + middle_norm
        }
    } else {
        middle_norm
    };

    if inverted { result } else { -result }
}

fn update_particles(
    particles: &mut [PhysicsParticle],
    total_translation: Vec2,
    total_angle: f32,
    wind: Vec2,
    threshold: f32,
    delta: f32,
) {
    const AIR_RESISTANCE: f32 = 5.0;

    particles[0].position = total_translation;
    let current_gravity = Vec2::new(
        total_angle.to_radians().sin(),
        total_angle.to_radians().cos(),
    )
    .normalized();

    for index in 1..particles.len() {
        let previous_position = particles[index - 1].position;
        let particle = &mut particles[index];
        particle.force = current_gravity * particle.acceleration + wind;
        particle.last_position = particle.position;

        let delay = particle.delay * delta * 30.0;
        let direction = (particle.position - previous_position)
            .rotate(direction_to_radian(particle.last_gravity, current_gravity) / AIR_RESISTANCE);

        particle.position = previous_position + direction;
        particle.position += particle.velocity * delay + particle.force * delay * delay;

        let new_direction = (particle.position - previous_position).normalized();
        particle.position = previous_position + new_direction * particle.radius;

        if particle.position.x.abs() < threshold {
            particle.position.x = 0.0;
        }

        if delay.abs() > f32::EPSILON {
            particle.velocity = (particle.position - particle.last_position) / delay;
            particle.velocity *= particle.mobility;
        }

        particle.force = Vec2::ZERO;
        particle.last_gravity = current_gravity;
    }
}

fn update_particles_for_stabilization(
    particles: &mut [PhysicsParticle],
    total_translation: Vec2,
    total_angle: f32,
    wind: Vec2,
    threshold: f32,
) {
    particles[0].position = total_translation;
    let current_gravity = Vec2::new(
        total_angle.to_radians().sin(),
        total_angle.to_radians().cos(),
    )
    .normalized();

    for index in 1..particles.len() {
        let previous_position = particles[index - 1].position;
        let particle = &mut particles[index];
        particle.force = current_gravity * particle.acceleration + wind;
        particle.last_position = particle.position;
        particle.velocity = Vec2::ZERO;

        let force = particle.force.normalized() * particle.radius;
        particle.position = previous_position + force;

        if particle.position.x.abs() < threshold {
            particle.position.x = 0.0;
        }

        particle.force = Vec2::ZERO;
        particle.last_gravity = current_gravity;
    }
}

fn output_value(
    kind: PhysicsValueKind,
    translation: Vec2,
    particles: &[PhysicsParticle],
    particle_index: usize,
    reflect: bool,
    parent_gravity: Vec2,
) -> f32 {
    let mut value = match kind {
        PhysicsValueKind::X => translation.x,
        PhysicsValueKind::Y => translation.y,
        PhysicsValueKind::Angle => {
            let parent = if particle_index >= 2 {
                particles[particle_index - 1].position - particles[particle_index - 2].position
            } else {
                parent_gravity * -1.0
            };
            direction_to_radian(parent, translation)
        }
    };

    if reflect {
        value *= -1.0;
    }

    value
}

fn apply_output_parameter(
    runtime: &mut CubismModelRuntime,
    parameter_id: &str,
    physics_value: f32,
    output: &PhysicsOutput,
) {
    let Some(parameter) = runtime.parameter(parameter_id) else {
        return;
    };
    let target = (physics_value * output.scale).clamp(parameter.min, parameter.max);
    let weight = output.weight.clamp(0.0, 1.0);
    let value = if weight >= 1.0 {
        target
    } else {
        parameter.value * (1.0 - weight) + target * weight
    };

    runtime.set_parameter_value(parameter_id, value);
}

fn direction_to_radian(from: Vec2, to: Vec2) -> f32 {
    let from = from.normalized();
    let to = to.normalized();
    if from.length() <= f32::EPSILON || to.length() <= f32::EPSILON {
        return 0.0;
    }

    let dot = (from.x * to.x + from.y * to.y).clamp(-1.0, 1.0);
    let cross = from.x * to.y - from.y * to.x;
    cross.atan2(dot)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsManifest {
    meta: Option<PhysicsMetaManifest>,
    #[serde(default)]
    physics_settings: Vec<PhysicsSettingManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsMetaManifest {
    fps: Option<f32>,
    effective_forces: Option<EffectiveForcesManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EffectiveForcesManifest {
    gravity: PhysicsVectorManifest,
    wind: PhysicsVectorManifest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsSettingManifest {
    #[serde(default)]
    input: Vec<PhysicsInputManifest>,
    #[serde(default)]
    output: Vec<PhysicsOutputManifest>,
    #[serde(default)]
    vertices: Vec<PhysicsVertexManifest>,
    normalization: Option<PhysicsNormalizationManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsInputManifest {
    source: PhysicsTarget,
    weight: f32,
    #[serde(rename = "Type")]
    kind: PhysicsValueKindManifest,
    reflect: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsOutputManifest {
    destination: PhysicsTarget,
    vertex_index: usize,
    scale: f32,
    weight: f32,
    #[serde(rename = "Type")]
    kind: PhysicsValueKindManifest,
    reflect: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsTarget {
    target: String,
    id: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum PhysicsValueKindManifest {
    X,
    Y,
    Angle,
}

impl From<PhysicsValueKindManifest> for PhysicsValueKind {
    fn from(kind: PhysicsValueKindManifest) -> Self {
        match kind {
            PhysicsValueKindManifest::X => Self::X,
            PhysicsValueKindManifest::Y => Self::Y,
            PhysicsValueKindManifest::Angle => Self::Angle,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsVertexManifest {
    position: PhysicsVectorManifest,
    mobility: f32,
    delay: f32,
    acceleration: f32,
    radius: f32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsVectorManifest {
    x: f32,
    y: f32,
}

impl From<PhysicsVectorManifest> for Vec2 {
    fn from(value: PhysicsVectorManifest) -> Self {
        Self::new(value.x, value.y)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsNormalizationManifest {
    position: PhysicsNormalizationRangeManifest,
    angle: PhysicsNormalizationRangeManifest,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PhysicsNormalizationRangeManifest {
    minimum: f32,
    default: f32,
    maximum: f32,
}

impl From<PhysicsNormalizationRangeManifest> for NormalizationRange {
    fn from(value: PhysicsNormalizationRangeManifest) -> Self {
        Self {
            min: value.minimum,
            default: value.default,
            max: value.maximum,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MotionManifest {
    meta: MotionMetaManifest,
    #[serde(default)]
    curves: Vec<MotionCurveManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MotionMetaManifest {
    duration: f32,
    #[serde(default)]
    #[serde(rename = "Loop")]
    looping: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MotionCurveManifest {
    target: String,
    id: String,
    #[serde(default)]
    segments: Vec<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExpressionManifest {
    #[serde(default)]
    parameters: Vec<ExpressionParameterManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExpressionParameterManifest {
    id: String,
    value: f32,
    blend: Option<ExpressionBlend>,
}

#[derive(Clone, Copy, Debug, Deserialize, Default)]
enum ExpressionBlend {
    #[default]
    Add,
    Multiply,
    Overwrite,
}
