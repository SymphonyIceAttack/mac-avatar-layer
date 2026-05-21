use crate::config::{AppConfig, CameraConfig, MicrophoneConfig, MouseConfig};
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
    camera_driver: Option<CameraDriver>,
    pose_input_mode: PoseInputMode,
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
    pub camera: Option<CameraMotionSample>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CameraMotionSample {
    pub face_offset: [f32; 2],
    pub face_angle: Option<[f32; 2]>,
    pub face_roll: f32,
    pub gaze: Option<[f32; 2]>,
    pub mouth_open: Option<f32>,
    pub eye_open: Option<f32>,
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
        let camera_driver = CameraDriver::from_config(&config.input.camera);
        let pose_input_mode =
            PoseInputMode::from_config(&config.input.camera.pose_mode, camera_driver.is_some());
        let mic_driver = MicMouthDriver::from_config(&config.input.microphone);

        Self {
            eye_blink_ids,
            idle_motion,
            expression,
            mouse_driver,
            camera_driver,
            pose_input_mode,
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

        let use_camera_pose = self.pose_input_mode.use_camera_pose(input.camera);
        if !use_camera_pose {
            if let Some(mouse_driver) = &mut self.mouse_driver {
                mouse_driver.apply(runtime, input.pointer, delta);
            }
        }

        let camera_mouth = self.camera_driver.as_mut().and_then(|camera_driver| {
            camera_driver.apply(runtime, input.camera, delta, use_camera_pose)
        });

        let mic_mouth = self
            .mic_driver
            .as_mut()
            .and_then(|mic_driver| mic_driver.update(input.mouth_level, delta));
        apply_mouth_value(
            runtime,
            mic_mouth,
            camera_mouth,
            self.camera_driver.as_ref(),
        );

        let neutralized_eye_values = if self.should_neutralize_blink_for_physics(use_camera_pose) {
            neutralize_eye_blink_for_physics(runtime, &self.eye_blink_ids)
        } else {
            Vec::new()
        };
        if let Some(physics) = &mut self.physics {
            physics.apply(runtime, delta);
        }
        restore_eye_blink_after_physics(runtime, neutralized_eye_values);

        if let Some(value) = self.mouth_open_override {
            runtime.set_parameter_value("ParamMouthOpenY", value);
        }
        if let Some(value) = self.mouth_form_override {
            runtime.set_parameter_value("ParamMouthForm", value);
        }

        runtime.update();
    }

    pub fn set_mouse_enabled(&mut self, enabled: bool, config: &MouseConfig) {
        self.mouse_driver = if enabled {
            Some(MouseDriver::from_runtime_config(config))
        } else {
            None
        };
    }

    pub fn set_microphone_enabled(&mut self, enabled: bool, config: &MicrophoneConfig) {
        self.mic_driver = if enabled {
            Some(MicMouthDriver::from_runtime_config(config))
        } else {
            None
        };
    }

    pub fn set_camera_config(&mut self, config: &CameraConfig) {
        if self.camera_driver.is_some() {
            self.camera_driver = Some(CameraDriver::from_runtime_config(config));
        }
        self.pose_input_mode =
            PoseInputMode::from_config(&config.pose_mode, self.camera_driver.is_some());
    }

    pub fn set_camera_enabled(&mut self, enabled: bool, config: &CameraConfig) {
        self.camera_driver = if enabled {
            Some(CameraDriver::from_runtime_config(config))
        } else {
            None
        };
        self.pose_input_mode =
            PoseInputMode::from_config(&config.pose_mode, self.camera_driver.is_some());
    }

    pub fn set_expression(&mut self, model: &Live2dModel, requested: Option<&str>) -> bool {
        self.expression = load_expression(model, requested);
        requested.is_none() || self.expression.is_some()
    }

    fn should_neutralize_blink_for_physics(&self, use_camera_pose: bool) -> bool {
        use_camera_pose
            && self
                .camera_driver
                .as_ref()
                .is_some_and(CameraDriver::drives_blink)
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
    dead_zone: f32,
    invert_x: bool,
    invert_y: bool,
    eye_x_range: f32,
    eye_y_range: f32,
    angle_x_degrees: f32,
    angle_y_degrees: f32,
    angle_z_degrees: f32,
}

impl MouseDriver {
    fn from_config(config: &MouseConfig) -> Option<Self> {
        config.enabled.then(|| {
            println!("Mouse tracking enabled: driving eye ball and head angle parameters");
            Self::from_runtime_config(config)
        })
    }

    fn from_runtime_config(config: &MouseConfig) -> Self {
        Self {
            eye_x: 0.0,
            eye_y: 0.0,
            angle_x: 0.0,
            angle_y: 0.0,
            angle_z: 0.0,
            smoothing: config.smoothing.clamp(1.0, 60.0),
            dead_zone: config.dead_zone.clamp(0.0, 0.95),
            invert_x: config.invert_x,
            invert_y: config.invert_y,
            eye_x_range: config.eye_x_range.clamp(0.0, 3.0),
            eye_y_range: config.eye_y_range.clamp(0.0, 3.0),
            angle_x_degrees: config.angle_x_degrees.clamp(-90.0, 90.0),
            angle_y_degrees: config.angle_y_degrees.clamp(-90.0, 90.0),
            angle_z_degrees: config.angle_z_degrees.clamp(-90.0, 90.0),
        }
    }

    fn apply(&mut self, runtime: &mut CubismModelRuntime, pointer: Option<[f32; 2]>, delta: f32) {
        let Some([x, y]) = pointer else {
            return;
        };
        let mut x = apply_dead_zone(x.clamp(-1.0, 1.0), self.dead_zone);
        let mut y = apply_dead_zone(y.clamp(-1.0, 1.0), self.dead_zone);
        if self.invert_x {
            x = -x;
        }
        if self.invert_y {
            y = -y;
        }
        let alpha = (1.0 - (-self.smoothing * delta).exp()).clamp(0.0, 1.0);

        self.eye_x = lerp(self.eye_x, x * self.eye_x_range, alpha);
        self.eye_y = lerp(self.eye_y, y * self.eye_y_range, alpha);
        self.angle_x = lerp(self.angle_x, x * self.angle_x_degrees, alpha);
        self.angle_y = lerp(self.angle_y, y * self.angle_y_degrees, alpha);
        self.angle_z = lerp(self.angle_z, x * self.angle_z_degrees, alpha);

        runtime.set_parameter_value("ParamEyeBallX", self.eye_x);
        runtime.set_parameter_value("ParamEyeBallY", self.eye_y);
        runtime.set_parameter_value("ParamAngleX", self.angle_x);
        runtime.set_parameter_value("ParamAngleY", self.angle_y);
        runtime.set_parameter_value("ParamAngleZ", self.angle_z);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PoseInputMode {
    Mouse,
    Camera,
    CameraWhenAvailable,
}

impl PoseInputMode {
    fn from_config(value: &str, camera_enabled: bool) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "mouse" => Self::Mouse,
            "camera" | "face" => Self::Camera,
            "camera_when_available" | "camera-when-available" | "auto" => Self::CameraWhenAvailable,
            _ if camera_enabled => Self::CameraWhenAvailable,
            _ => Self::Mouse,
        }
    }

    fn use_camera_pose(self, sample: Option<CameraMotionSample>) -> bool {
        match self {
            Self::Mouse => false,
            Self::Camera => true,
            Self::CameraWhenAvailable => sample.is_some(),
        }
    }
}

struct CameraDriver {
    eye_x: f32,
    eye_y: f32,
    angle_x: f32,
    angle_y: f32,
    angle_z: f32,
    mouth_open: f32,
    blink_eye_open: f32,
    blink_closed: bool,
    smoothing: f32,
    dead_zone: f32,
    invert_x: bool,
    invert_y: bool,
    calibration: CameraCalibration,
    eye_x_range: f32,
    eye_y_range: f32,
    angle_x_degrees: f32,
    angle_y_degrees: f32,
    angle_z_degrees: f32,
    mouth_enabled: bool,
    mouth_gain: f32,
    mouth_open_offset: f32,
    mouth_min_open: f32,
    mouth_max_open: f32,
    mouth_combine: MouthCombineMode,
    blink_from_camera: bool,
    blink_close_threshold: f32,
    blink_open_threshold: f32,
}

impl CameraDriver {
    fn from_config(config: &CameraConfig) -> Option<Self> {
        config.enabled.then(|| {
            println!("Camera tracking enabled: waiting for native face samples");
            Self::from_runtime_config(config)
        })
    }

    fn from_runtime_config(config: &CameraConfig) -> Self {
        Self {
            eye_x: 0.0,
            eye_y: 0.0,
            angle_x: 0.0,
            angle_y: 0.0,
            angle_z: 0.0,
            mouth_open: 0.0,
            blink_eye_open: 1.0,
            blink_closed: false,
            smoothing: config.smoothing.clamp(1.0, 60.0),
            dead_zone: config.dead_zone.clamp(0.0, 0.95),
            invert_x: config.invert_x,
            invert_y: config.invert_y,
            calibration: CameraCalibration::from_config(config),
            eye_x_range: config.eye_x_range.clamp(0.0, 3.0),
            eye_y_range: config.eye_y_range.clamp(0.0, 3.0),
            angle_x_degrees: config.angle_x_degrees.clamp(-90.0, 90.0),
            angle_y_degrees: config.angle_y_degrees.clamp(-90.0, 90.0),
            angle_z_degrees: config.angle_z_degrees.clamp(-90.0, 90.0),
            mouth_enabled: config.mouth_enabled,
            mouth_gain: config.mouth_gain.clamp(0.1, 10.0),
            mouth_open_offset: config.mouth_open_offset.clamp(-1.0, 1.0),
            mouth_min_open: config.mouth_min_open.clamp(0.0, 1.0),
            mouth_max_open: config.mouth_max_open.clamp(0.0, 1.0),
            mouth_combine: MouthCombineMode::from_config(&config.mouth_combine),
            blink_from_camera: config.blink_from_camera,
            blink_close_threshold: config.blink_close_threshold.clamp(0.0, 1.0),
            blink_open_threshold: config.blink_open_threshold.clamp(0.0, 1.0),
        }
    }

    fn apply(
        &mut self,
        runtime: &mut CubismModelRuntime,
        sample: Option<CameraMotionSample>,
        delta: f32,
        apply_pose: bool,
    ) -> Option<f32> {
        let alpha = (1.0 - (-self.smoothing * delta).exp()).clamp(0.0, 1.0);

        if apply_pose {
            let target = camera_pose_target(
                sample,
                self.dead_zone,
                self.invert_x,
                self.invert_y,
                self.calibration,
            );
            self.eye_x = lerp(self.eye_x, target.eye_x * self.eye_x_range, alpha);
            self.eye_y = lerp(self.eye_y, target.eye_y * self.eye_y_range, alpha);
            self.angle_x = lerp(self.angle_x, target.angle_x * self.angle_x_degrees, alpha);
            self.angle_y = lerp(self.angle_y, target.angle_y * self.angle_y_degrees, alpha);
            self.angle_z = lerp(self.angle_z, target.angle_z * self.angle_z_degrees, alpha);

            runtime.set_parameter_value("ParamEyeBallX", self.eye_x);
            runtime.set_parameter_value("ParamEyeBallY", self.eye_y);
            runtime.set_parameter_value("ParamAngleX", self.angle_x);
            runtime.set_parameter_value("ParamAngleY", self.angle_y);
            runtime.set_parameter_value("ParamAngleZ", self.angle_z);
        }

        if apply_pose && self.blink_from_camera {
            if let Some(eye_open) = self.update_blink(sample, alpha) {
                runtime.set_parameter_value("ParamEyeLOpen", eye_open);
                runtime.set_parameter_value("ParamEyeROpen", eye_open);
            }
        }

        self.update_mouth(sample, alpha)
    }

    fn update_mouth(&mut self, sample: Option<CameraMotionSample>, alpha: f32) -> Option<f32> {
        if !self.mouth_enabled {
            return None;
        }

        let target =
            sample.and_then(|sample| sample.mouth_open).unwrap_or(0.0) + self.mouth_open_offset;
        let target = target.clamp(0.0, 1.0) * self.mouth_gain;
        let min_open = self.mouth_min_open.min(self.mouth_max_open);
        let max_open = self.mouth_min_open.max(self.mouth_max_open);
        let target = min_open + target.clamp(0.0, 1.0) * (max_open - min_open);
        self.mouth_open = lerp(self.mouth_open, target, alpha);
        Some(self.mouth_open)
    }

    fn update_blink(&mut self, sample: Option<CameraMotionSample>, alpha: f32) -> Option<f32> {
        let raw = sample.and_then(|sample| sample.eye_open)?.clamp(0.0, 1.0);
        let close_threshold = self.blink_close_threshold.min(self.blink_open_threshold);
        let open_threshold = self.blink_close_threshold.max(self.blink_open_threshold);

        if raw <= close_threshold {
            self.blink_closed = true;
        } else if raw >= open_threshold {
            self.blink_closed = false;
        }

        let target = if raw <= close_threshold {
            0.0
        } else if open_threshold > close_threshold {
            ((raw - close_threshold) / (open_threshold - close_threshold)).clamp(0.0, 1.0)
        } else {
            raw
        };
        self.blink_eye_open = lerp(self.blink_eye_open, target, alpha);
        Some(self.blink_eye_open)
    }

    fn mouth_combine(&self) -> MouthCombineMode {
        self.mouth_combine
    }

    fn drives_blink(&self) -> bool {
        self.blink_from_camera
    }
}

fn neutralize_eye_blink_for_physics(
    runtime: &mut CubismModelRuntime,
    eye_blink_ids: &[String],
) -> Vec<(String, f32)> {
    let mut previous_values = Vec::new();
    for id in eye_blink_ids {
        if let Some(parameter) = runtime.parameter(id) {
            previous_values.push((id.clone(), parameter.value));
            runtime.set_parameter_value(id, 1.0);
        }
    }
    previous_values
}

fn restore_eye_blink_after_physics(
    runtime: &mut CubismModelRuntime,
    previous_values: Vec<(String, f32)>,
) {
    for (id, value) in previous_values {
        runtime.set_parameter_value(&id, value);
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CameraCalibration {
    face_x_offset: f32,
    face_y_offset: f32,
    gaze_x_offset: f32,
    gaze_y_offset: f32,
    roll_offset: f32,
}

impl CameraCalibration {
    fn from_config(config: &CameraConfig) -> Self {
        Self {
            face_x_offset: config.face_x_offset.clamp(-1.0, 1.0),
            face_y_offset: config.face_y_offset.clamp(-1.0, 1.0),
            gaze_x_offset: config.gaze_x_offset.clamp(-1.0, 1.0),
            gaze_y_offset: config.gaze_y_offset.clamp(-1.0, 1.0),
            roll_offset: config.roll_offset.clamp(-1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MouthCombineMode {
    Max,
    Camera,
    Microphone,
}

impl MouthCombineMode {
    fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "camera" => Self::Camera,
            "microphone" | "mic" => Self::Microphone,
            _ => Self::Max,
        }
    }

    fn combine(self, camera: f32, microphone: f32) -> f32 {
        match self {
            Self::Max => camera.max(microphone),
            Self::Camera => camera,
            Self::Microphone => microphone,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CameraPoseTarget {
    eye_x: f32,
    eye_y: f32,
    angle_x: f32,
    angle_y: f32,
    angle_z: f32,
}

fn camera_pose_target(
    sample: Option<CameraMotionSample>,
    dead_zone: f32,
    invert_x: bool,
    invert_y: bool,
    calibration: CameraCalibration,
) -> CameraPoseTarget {
    let Some(sample) = sample else {
        return CameraPoseTarget {
            eye_x: 0.0,
            eye_y: 0.0,
            angle_x: 0.0,
            angle_y: 0.0,
            angle_z: 0.0,
        };
    };

    let offset_calibrated = [
        (sample.face_offset[0] + calibration.face_x_offset).clamp(-1.0, 1.0),
        (sample.face_offset[1] + calibration.face_y_offset).clamp(-1.0, 1.0),
    ];
    let angle_source = sample.face_angle.unwrap_or(sample.face_offset);
    let face_raw = [
        (angle_source[0] + calibration.face_x_offset).clamp(-1.0, 1.0),
        (angle_source[1] + calibration.face_y_offset).clamp(-1.0, 1.0),
    ];
    let mut face_x = apply_dead_zone(face_raw[0], dead_zone);
    let mut face_y = apply_dead_zone(face_raw[1], dead_zone);
    if invert_x {
        face_x = -face_x;
    }
    if invert_y {
        face_y = -face_y;
    }

    let gaze = sample.gaze.unwrap_or(offset_calibrated);
    let mut gaze_x = apply_dead_zone(
        (gaze[0] + calibration.gaze_x_offset).clamp(-1.0, 1.0),
        dead_zone,
    );
    let mut gaze_y = apply_dead_zone(
        (gaze[1] + calibration.gaze_y_offset).clamp(-1.0, 1.0),
        dead_zone,
    );
    if invert_x {
        gaze_x = -gaze_x;
    }
    if invert_y {
        gaze_y = -gaze_y;
    }

    CameraPoseTarget {
        eye_x: gaze_x,
        eye_y: gaze_y,
        angle_x: face_x,
        angle_y: face_y,
        angle_z: (sample.face_roll + calibration.roll_offset).clamp(-1.0, 1.0),
    }
}

struct MicMouthDriver {
    parameter: String,
    value: f32,
    gain: f32,
    noise_gate: f32,
    response_curve: f32,
    attack: f32,
    release: f32,
    min_open: f32,
    max_open: f32,
}

impl MicMouthDriver {
    fn from_config(config: &MicrophoneConfig) -> Option<Self> {
        config.enabled.then(|| Self::from_runtime_config(config))
    }

    fn from_runtime_config(config: &MicrophoneConfig) -> Self {
        let parameter = config.parameter.trim();
        Self {
            parameter: if parameter.is_empty() {
                "ParamMouthOpenY".to_string()
            } else {
                parameter.to_string()
            },
            value: 0.0,
            gain: config.gain.clamp(0.1, 80.0),
            noise_gate: config.noise_gate.clamp(0.0, 0.5),
            response_curve: config.response_curve.clamp(0.2, 3.0),
            attack: positive_or(config.attack, config.smoothing).clamp(1.0, 120.0),
            release: positive_or(config.release, config.smoothing).clamp(1.0, 120.0),
            min_open: config.min_open.clamp(0.0, 1.0),
            max_open: config.max_open.clamp(0.0, 1.0),
        }
    }

    fn update(&mut self, level: Option<f32>, delta: f32) -> Option<MouthValue> {
        let level = level?;
        let level = level.clamp(0.0, 1.0);
        let mut target = if level <= self.noise_gate {
            0.0
        } else {
            let normalized = ((level - self.noise_gate) / (1.0 - self.noise_gate)).clamp(0.0, 1.0);
            normalized.powf(self.response_curve) * self.gain
        }
        .clamp(0.0, 1.0);
        let min_open = self.min_open.min(self.max_open);
        let max_open = self.min_open.max(self.max_open);
        target = min_open + target * (max_open - min_open);
        let smoothing = if target > self.value {
            self.attack
        } else {
            self.release
        };
        let alpha = (1.0 - (-smoothing * delta).exp()).clamp(0.0, 1.0);
        self.value = lerp(self.value, target, alpha);
        Some(MouthValue {
            parameter: self.parameter.clone(),
            value: self.value,
        })
    }
}

#[derive(Debug, Clone)]
struct MouthValue {
    parameter: String,
    value: f32,
}

fn apply_mouth_value(
    runtime: &mut CubismModelRuntime,
    microphone: Option<MouthValue>,
    camera: Option<f32>,
    camera_driver: Option<&CameraDriver>,
) {
    const CAMERA_MOUTH_PARAMETER: &str = "ParamMouthOpenY";

    let Some(camera) = camera else {
        if let Some(microphone) = microphone {
            runtime.set_parameter_value(&microphone.parameter, microphone.value);
        }
        return;
    };

    let Some(microphone) = microphone else {
        runtime.set_parameter_value(CAMERA_MOUTH_PARAMETER, camera);
        return;
    };

    if microphone.parameter != CAMERA_MOUTH_PARAMETER {
        runtime.set_parameter_value(&microphone.parameter, microphone.value);
        runtime.set_parameter_value(CAMERA_MOUTH_PARAMETER, camera);
        return;
    }

    let combine_mode = camera_driver
        .map(CameraDriver::mouth_combine)
        .unwrap_or(MouthCombineMode::Max);
    let value = combine_mode.combine(camera, microphone.value);
    runtime.set_parameter_value(CAMERA_MOUTH_PARAMETER, value);
}

fn lerp(current: f32, target: f32, alpha: f32) -> f32 {
    current + (target - current) * alpha
}

fn apply_dead_zone(value: f32, dead_zone: f32) -> f32 {
    let dead_zone = dead_zone.clamp(0.0, 0.95);
    let magnitude = value.abs();
    if magnitude <= dead_zone {
        0.0
    } else {
        value.signum() * ((magnitude - dead_zone) / (1.0 - dead_zone)).clamp(0.0, 1.0)
    }
}

fn positive_or(value: f32, fallback: f32) -> f32 {
    if value > 0.0 { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::{
        CameraCalibration, CameraDriver, CameraMotionSample, MouthCombineMode, PoseInputMode,
        apply_dead_zone, camera_pose_target, positive_or,
    };
    use crate::config::CameraConfig;

    #[test]
    fn dead_zone_zeroes_center_and_rescales_remaining_range() {
        assert_eq!(apply_dead_zone(0.04, 0.05), 0.0);
        assert_eq!(apply_dead_zone(-0.04, 0.05), 0.0);

        let value = apply_dead_zone(0.525, 0.05);
        assert!((value - 0.5).abs() < 0.0001);
        let value = apply_dead_zone(-0.525, 0.05);
        assert!((value + 0.5).abs() < 0.0001);
    }

    #[test]
    fn positive_or_uses_fallback_for_non_positive_values() {
        assert_eq!(positive_or(12.0, 18.0), 12.0);
        assert_eq!(positive_or(0.0, 18.0), 18.0);
        assert_eq!(positive_or(-1.0, 18.0), 18.0);
    }

    #[test]
    fn camera_pose_uses_gaze_when_available_and_face_for_head_angles() {
        let target = camera_pose_target(
            Some(CameraMotionSample {
                face_offset: [0.52, -0.33],
                face_angle: None,
                face_roll: 0.25,
                gaze: Some([-0.42, 0.24]),
                mouth_open: None,
                eye_open: None,
            }),
            0.02,
            false,
            false,
            CameraCalibration::default(),
        );

        assert!(target.angle_x > 0.5);
        assert!(target.angle_y < -0.3);
        assert!(target.eye_x < -0.4);
        assert!(target.eye_y > 0.2);
        assert_eq!(target.angle_z, 0.25);
    }

    #[test]
    fn camera_pose_decays_to_neutral_without_sample() {
        let target = camera_pose_target(None, 0.02, false, false, CameraCalibration::default());

        assert_eq!(target.angle_x, 0.0);
        assert_eq!(target.angle_y, 0.0);
        assert_eq!(target.angle_z, 0.0);
        assert_eq!(target.eye_x, 0.0);
        assert_eq!(target.eye_y, 0.0);
    }

    #[test]
    fn camera_calibration_offsets_adjust_pose_targets() {
        let target = camera_pose_target(
            Some(CameraMotionSample {
                face_offset: [0.10, -0.10],
                face_angle: None,
                face_roll: 0.10,
                gaze: Some([0.20, -0.20]),
                mouth_open: None,
                eye_open: None,
            }),
            0.0,
            false,
            false,
            CameraCalibration {
                face_x_offset: 0.25,
                face_y_offset: -0.15,
                gaze_x_offset: -0.10,
                gaze_y_offset: 0.30,
                roll_offset: 0.20,
            },
        );

        assert!((target.angle_x - 0.35).abs() < 0.001);
        assert!((target.angle_y + 0.25).abs() < 0.001);
        assert!((target.eye_x - 0.10).abs() < 0.001);
        assert!((target.eye_y - 0.10).abs() < 0.001);
        assert!((target.angle_z - 0.30).abs() < 0.001);
    }

    #[test]
    fn camera_pose_prefers_face_angles_over_face_center_for_head_rotation() {
        let target = camera_pose_target(
            Some(CameraMotionSample {
                face_offset: [0.80, -0.80],
                face_angle: Some([-0.20, 0.30]),
                face_roll: 0.0,
                gaze: None,
                mouth_open: None,
                eye_open: None,
            }),
            0.0,
            false,
            false,
            CameraCalibration::default(),
        );

        assert!((target.angle_x + 0.20).abs() < 0.001);
        assert!((target.angle_y - 0.30).abs() < 0.001);
        assert_eq!(target.eye_x, 0.80);
        assert_eq!(target.eye_y, -0.80);
    }

    #[test]
    fn camera_blink_uses_threshold_hysteresis() {
        let config = CameraConfig {
            enabled: true,
            blink_from_camera: true,
            blink_close_threshold: 0.20,
            blink_open_threshold: 0.38,
            smoothing: 60.0,
            ..CameraConfig::default()
        };
        let mut driver = CameraDriver::from_runtime_config(&config);
        let closed = driver.update_blink(
            Some(CameraMotionSample {
                face_offset: [0.0, 0.0],
                face_angle: None,
                face_roll: 0.0,
                gaze: None,
                mouth_open: None,
                eye_open: Some(0.10),
            }),
            1.0,
        );
        assert_eq!(closed, Some(0.0));

        let reopening = driver.update_blink(
            Some(CameraMotionSample {
                face_offset: [0.0, 0.0],
                face_angle: None,
                face_roll: 0.0,
                gaze: None,
                mouth_open: None,
                eye_open: Some(0.29),
            }),
            1.0,
        );
        assert!(reopening.unwrap() > 0.0);
        assert!(driver.blink_closed);

        let open = driver.update_blink(
            Some(CameraMotionSample {
                face_offset: [0.0, 0.0],
                face_angle: None,
                face_roll: 0.0,
                gaze: None,
                mouth_open: None,
                eye_open: Some(0.45),
            }),
            1.0,
        );
        assert!(open.unwrap() > 0.9);
        assert!(!driver.blink_closed);
    }

    #[test]
    fn camera_blink_marks_eye_inputs_for_physics_neutralization() {
        let enabled = CameraConfig {
            enabled: true,
            blink_from_camera: true,
            ..CameraConfig::default()
        };
        let disabled = CameraConfig {
            enabled: true,
            blink_from_camera: false,
            ..CameraConfig::default()
        };

        assert!(CameraDriver::from_runtime_config(&enabled).drives_blink());
        assert!(!CameraDriver::from_runtime_config(&disabled).drives_blink());
    }

    #[test]
    fn camera_mouth_combine_modes_are_stable() {
        assert_eq!(MouthCombineMode::from_config("max"), MouthCombineMode::Max);
        assert_eq!(
            MouthCombineMode::from_config("camera"),
            MouthCombineMode::Camera
        );
        assert_eq!(
            MouthCombineMode::from_config("mic"),
            MouthCombineMode::Microphone
        );
        assert_eq!(
            MouthCombineMode::from_config("unknown"),
            MouthCombineMode::Max
        );

        assert_eq!(MouthCombineMode::Max.combine(0.3, 0.7), 0.7);
        assert_eq!(MouthCombineMode::Camera.combine(0.3, 0.7), 0.3);
        assert_eq!(MouthCombineMode::Microphone.combine(0.3, 0.7), 0.7);
    }

    #[test]
    fn pose_input_mode_avoids_mouse_camera_parameter_fights() {
        let sample = Some(CameraMotionSample {
            face_offset: [0.1, 0.2],
            face_angle: None,
            face_roll: 0.0,
            gaze: None,
            mouth_open: None,
            eye_open: None,
        });

        assert!(!PoseInputMode::from_config("mouse", true).use_camera_pose(sample));
        assert!(PoseInputMode::from_config("camera", true).use_camera_pose(None));
        assert!(PoseInputMode::from_config("auto", true).use_camera_pose(sample));
        assert!(!PoseInputMode::from_config("auto", true).use_camera_pose(None));
        assert!(!PoseInputMode::from_config("", false).use_camera_pose(sample));
    }
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
