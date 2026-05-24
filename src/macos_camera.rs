#![cfg(all(target_os = "macos", feature = "camera-tracking"))]

use crate::camera_input::CameraStatus;
use crate::config::CameraConfig;
use crate::motion::CameraMotionSample;
use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, extern_methods};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput,
    AVCaptureOutput, AVCaptureSession, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaType, AVMediaTypeVideo,
};
use objc2_core_media::CMSampleBuffer;
use objc2_foundation::{NSArray, NSError};
use objc2_vision::{
    VNDetectFaceLandmarksRequest, VNDetectFaceRectanglesRequest,
    VNDetectFaceRectanglesRequestRevision3, VNFaceLandmarkRegion2D, VNFaceObservation, VNRequest,
    VNSequenceRequestHandler,
};
use std::f32::consts::{FRAC_PI_4, FRAC_PI_6};
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static SAMPLE_BUFFER_CALLBACKS: AtomicU64 = AtomicU64::new(0);
static DROPPED_SAMPLE_BUFFER_CALLBACKS: AtomicU64 = AtomicU64::new(0);
static VISION_MIN_INTERVAL_NS: AtomicU64 = AtomicU64::new(33_333_333);
static VISION_LAST_ATTEMPT_NS: AtomicU64 = AtomicU64::new(0);
static VISION_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static VISION_FAILURES: AtomicU64 = AtomicU64::new(0);
static VISION_NO_FACE_RESULTS: AtomicU64 = AtomicU64::new(0);
static VISION_FACE_RESULTS: AtomicU64 = AtomicU64::new(0);
static LATEST_CAMERA_SAMPLE: Mutex<Option<CameraMotionSample>> = Mutex::new(None);

const FACE_YAW_NORMALIZATION_RADIANS: f32 = FRAC_PI_6;
const FACE_PITCH_NORMALIZATION_RADIANS: f32 = FRAC_PI_6;
const FACE_ROLL_NORMALIZATION_RADIANS: f32 = FRAC_PI_4;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "VtubeStudioRsCameraSampleBufferDelegate"]
    struct CameraSampleBufferDelegate;

    unsafe impl NSObjectProtocol for CameraSampleBufferDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CameraSampleBufferDelegate {}

    impl CameraSampleBufferDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn capture_output(
            &self,
            _output: &AVCaptureOutput,
            _sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            SAMPLE_BUFFER_CALLBACKS.fetch_add(1, Ordering::Relaxed);
            if should_process_vision_frame() {
                process_vision_frame(_sample_buffer);
            }
        }

        #[unsafe(method(captureOutput:didDropSampleBuffer:fromConnection:))]
        fn capture_dropped_output(
            &self,
            _output: &AVCaptureOutput,
            _sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            DROPPED_SAMPLE_BUFFER_CALLBACKS.fetch_add(1, Ordering::Relaxed);
        }
    }
);

impl CameraSampleBufferDelegate {
    extern_methods!(
        #[unsafe(method(new))]
        fn new() -> Retained<Self>;
    );
}

#[allow(dead_code)]
struct CameraCapturePipeline {
    session: Retained<AVCaptureSession>,
    input: Retained<AVCaptureDeviceInput>,
    output: Retained<AVCaptureVideoDataOutput>,
    delegate: Retained<CameraSampleBufferDelegate>,
    queue: DispatchRetained<DispatchQueue>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CameraProbe {
    pub status: CameraStatus,
    pub diagnostic: Option<String>,
}

pub struct CameraRuntime {
    pipeline: CameraCapturePipeline,
    diagnostic: String,
}

impl CameraRuntime {
    pub fn start(config: &CameraConfig) -> Result<Self, CameraProbe> {
        match setup_camera(config, true) {
            Ok(setup) => Ok(Self {
                pipeline: setup.pipeline,
                diagnostic: crate::apple_platform::local_only_camera_message(
                    "Camera permission, device probing, capture session configuration, sample buffer delegate wiring, capture session startup, Vision landmark sampling, and first-pass landmark-to-parameter mapping succeeded.",
                ),
            }),
            Err(probe) => Err(probe),
        }
    }

    pub fn status(&self) -> CameraStatus {
        if !self.pipeline.is_running() {
            CameraStatus::Failed
        } else if VISION_ATTEMPTS.load(Ordering::Relaxed) > 0
            && VISION_FACE_RESULTS.load(Ordering::Relaxed) == 0
        {
            CameraStatus::NoFace
        } else {
            CameraStatus::Running
        }
    }

    pub fn diagnostic(&self) -> Option<&str> {
        Some(&self.diagnostic)
    }

    pub fn latest_sample(&self) -> Option<CameraMotionSample> {
        latest_camera_sample()
    }
}

struct CameraSetup {
    pipeline: CameraCapturePipeline,
}

fn setup_camera(config: &CameraConfig, start_session: bool) -> Result<CameraSetup, CameraProbe> {
    let media_type_video = video_media_type().map_err(|error| CameraProbe {
        status: CameraStatus::Failed,
        diagnostic: Some(crate::apple_platform::local_only_camera_message(&format!(
            "Camera tracking setup failed: {error}"
        ))),
    })?;
    let auth_status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type_video) };

    match auth_status {
        AVAuthorizationStatus::NotDetermined => match request_camera_access(media_type_video) {
            Ok(true) => {}
            Ok(false) => {
                return Err(CameraProbe {
                    status: CameraStatus::PermissionDenied,
                    diagnostic: Some(crate::apple_platform::local_only_camera_message(
                        "Camera permission was denied. Enable camera access for MacAvatarLayer Dev in System Settings > Privacy & Security > Camera.",
                    )),
                });
            }
            Err(error) => {
                return Err(CameraProbe {
                    status: CameraStatus::WaitingForPermission,
                    diagnostic: Some(crate::apple_platform::local_only_camera_message(&format!(
                        "Camera permission has not been granted yet. macOS has been asked for access, but no response arrived before startup continued: {error}. Approve camera access for MacAvatarLayer Dev, then run the app again if the camera did not start.",
                    ))),
                });
            }
        },
        AVAuthorizationStatus::Restricted | AVAuthorizationStatus::Denied => {
            return Err(CameraProbe {
                status: CameraStatus::PermissionDenied,
                diagnostic: Some(crate::apple_platform::local_only_camera_message(
                    "Camera permission is denied or restricted. Enable camera access for the terminal/app that launches MacAvatarLayer in System Settings > Privacy & Security > Camera.",
                )),
            });
        }
        AVAuthorizationStatus::Authorized => {}
        other => {
            return Err(CameraProbe {
                status: CameraStatus::Failed,
                diagnostic: Some(format!(
                    "Unexpected AVFoundation camera authorization status: {}",
                    other.0
                )),
            });
        }
    }

    let device = if config.device.trim().is_empty() {
        default_video_device(media_type_video)
    } else {
        named_video_device(media_type_video, config.device.trim())
    };

    let Some(device) = device else {
        return Err(CameraProbe {
            status: CameraStatus::NoCamera,
            diagnostic: Some(crate::apple_platform::local_only_camera_message(
                "No matching macOS camera was found. Check that a webcam is connected and available to AVFoundation.",
            )),
        });
    };

    configure_vision_throttle(config.target_fps);
    let pipeline = build_capture_session(&device).map_err(|error| CameraProbe {
        status: CameraStatus::Failed,
        diagnostic: Some(crate::apple_platform::local_only_camera_message(&format!(
            "Camera tracking setup failed: {error}"
        ))),
    })?;
    if start_session {
        pipeline.start();
        if !pipeline.is_running() {
            return Err(CameraProbe {
                status: CameraStatus::Failed,
                diagnostic: Some(crate::apple_platform::local_only_camera_message(
                    "AVCaptureSession did not report running after startup.",
                )),
            });
        }
        println!("renderer_event=camera_capture_started");
    }

    Ok(CameraSetup { pipeline })
}

fn video_media_type() -> Result<&'static AVMediaType, String> {
    unsafe { AVMediaTypeVideo.ok_or_else(|| "AVMediaTypeVideo is unavailable".to_string()) }
}

fn request_camera_access(media_type_video: &AVMediaType) -> Result<bool, String> {
    let (sender, receiver) = mpsc::channel();
    let handler = RcBlock::new(move |granted: objc2::runtime::Bool| {
        let granted = granted.as_bool();
        println!(
            "renderer_event=camera_permission_response granted={}",
            granted
        );
        let _ = sender.send(granted);
    });
    unsafe {
        AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type_video, &handler);
    }
    receiver
        .recv_timeout(Duration::from_secs(60))
        .map_err(|error| error.to_string())
}

impl CameraCapturePipeline {
    fn start(&self) {
        unsafe {
            self.session.startRunning();
        }
    }

    fn is_running(&self) -> bool {
        unsafe { self.session.isRunning() }
    }
}

impl Drop for CameraCapturePipeline {
    fn drop(&mut self) {
        unsafe {
            if self.session.isRunning() {
                self.session.stopRunning();
                println!("renderer_event=camera_capture_stopped");
            }
            self.output.setSampleBufferDelegate_queue(None, None);
        }
    }
}

fn default_video_device(media_type_video: &AVMediaType) -> Option<Retained<AVCaptureDevice>> {
    unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media_type_video) }
}

fn named_video_device(
    media_type_video: &AVMediaType,
    requested_name: &str,
) -> Option<Retained<AVCaptureDevice>> {
    #[allow(deprecated)]
    let devices = unsafe { AVCaptureDevice::devicesWithMediaType(media_type_video) };
    for index in 0..devices.len() {
        let device = devices.objectAtIndex(index);
        if crate::apple_platform::foundation_string(&unsafe { device.localizedName() })
            == requested_name
        {
            return Some(device);
        }
    }

    None
}

fn build_capture_session(device: &AVCaptureDevice) -> Result<CameraCapturePipeline, String> {
    let session = unsafe { AVCaptureSession::new() };
    let input =
        unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(device) }.map_err(|error| {
            format!(
                "Failed to create AVCaptureDeviceInput: {}",
                crate::apple_platform::ns_error_description(error)
            )
        })?;
    let output = unsafe { AVCaptureVideoDataOutput::new() };
    let delegate = CameraSampleBufferDelegate::new();
    let queue = DispatchQueue::new(
        "io.github.symphonyiceattack.mac-avatar-layer.camera.samples",
        None,
    );

    unsafe {
        output.setAlwaysDiscardsLateVideoFrames(true);
        output.setSampleBufferDelegate_queue(
            Some(ProtocolObject::from_ref(delegate.deref())),
            Some(queue.deref()),
        );
    }

    unsafe {
        session.beginConfiguration();
    }
    let result = configure_capture_session(&session, &input, &output);
    unsafe {
        session.commitConfiguration();
    }
    result?;

    Ok(CameraCapturePipeline {
        session,
        input,
        output,
        delegate,
        queue,
    })
}

fn configure_vision_throttle(target_fps: u32) {
    let fps = target_fps.clamp(1, 60) as u64;
    VISION_MIN_INTERVAL_NS.store(1_000_000_000 / fps, Ordering::Relaxed);
    VISION_LAST_ATTEMPT_NS.store(0, Ordering::Relaxed);
    VISION_ATTEMPTS.store(0, Ordering::Relaxed);
    VISION_FAILURES.store(0, Ordering::Relaxed);
    VISION_NO_FACE_RESULTS.store(0, Ordering::Relaxed);
    VISION_FACE_RESULTS.store(0, Ordering::Relaxed);
    set_latest_camera_sample(None);
}

fn should_process_vision_frame() -> bool {
    let now = monotonicish_time_ns();
    let min_interval = VISION_MIN_INTERVAL_NS.load(Ordering::Relaxed);
    let previous = VISION_LAST_ATTEMPT_NS.load(Ordering::Relaxed);
    if previous != 0 && now.saturating_sub(previous) < min_interval {
        return false;
    }

    VISION_LAST_ATTEMPT_NS
        .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

fn monotonicish_time_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn process_vision_frame(sample_buffer: &CMSampleBuffer) {
    VISION_ATTEMPTS.fetch_add(1, Ordering::Relaxed);

    let handler = unsafe { VNSequenceRequestHandler::new() };
    let pose_result = perform_face_rectangles_request(&handler, sample_buffer);
    let landmarks_result = perform_face_landmarks_request(&handler, sample_buffer);

    let (pose_face_count, pose_sample) = match pose_result {
        Ok(result) => result,
        Err(error) => {
            log_vision_request_error("face rectangles", error);
            (0, None)
        }
    };
    let (landmark_face_count, landmark_sample) = match landmarks_result {
        Ok(result) => result,
        Err(error) => {
            log_vision_request_error("face landmarks", error);
            (0, None)
        }
    };

    let face_count = landmark_face_count.max(pose_face_count);
    if face_count > 0 {
        if let Some(sample) = merge_pose_sample(landmark_sample, pose_sample) {
            set_latest_camera_sample(Some(sample));
        }
        let previous = VISION_FACE_RESULTS.fetch_add(face_count, Ordering::Relaxed);
        if previous == 0 {
            println!("renderer_event=camera_vision_face_detected count={face_count}");
        }
    } else {
        set_latest_camera_sample(None);
        let previous = VISION_NO_FACE_RESULTS.fetch_add(1, Ordering::Relaxed);
        if previous == 0 {
            println!("renderer_event=camera_vision_no_face");
        }
    }
}

fn perform_face_rectangles_request(
    handler: &VNSequenceRequestHandler,
    sample_buffer: &CMSampleBuffer,
) -> Result<(u64, Option<CameraMotionSample>), Retained<NSError>> {
    let request = unsafe { VNDetectFaceRectanglesRequest::new() };
    unsafe {
        request.setRevision(VNDetectFaceRectanglesRequestRevision3);
        request.setPreferBackgroundProcessing(true);
    }
    let requests = NSArray::from_slice(&[request.deref()]);
    let requests = unsafe { requests.cast_unchecked::<VNRequest>() };
    unsafe { handler.performRequests_onCMSampleBuffer_error(requests, sample_buffer) }?;

    Ok(unsafe { request.results() }
        .as_deref()
        .map(first_sample_from_results)
        .unwrap_or((0, None)))
}

fn perform_face_landmarks_request(
    handler: &VNSequenceRequestHandler,
    sample_buffer: &CMSampleBuffer,
) -> Result<(u64, Option<CameraMotionSample>), Retained<NSError>> {
    let request = unsafe { VNDetectFaceLandmarksRequest::new() };
    unsafe {
        request.setPreferBackgroundProcessing(true);
    }
    let requests = NSArray::from_slice(&[request.deref()]);
    let requests = unsafe { requests.cast_unchecked::<VNRequest>() };
    unsafe { handler.performRequests_onCMSampleBuffer_error(requests, sample_buffer) }?;

    Ok(unsafe { request.results() }
        .as_deref()
        .map(first_sample_from_results)
        .unwrap_or((0, None)))
}

fn merge_pose_sample(
    landmark_sample: Option<CameraMotionSample>,
    pose_sample: Option<CameraMotionSample>,
) -> Option<CameraMotionSample> {
    match (landmark_sample, pose_sample) {
        (Some(mut landmark), Some(pose)) => {
            if landmark.face_angle.is_none() {
                landmark.face_angle = pose.face_angle;
            }
            landmark.face_roll = pose.face_roll;
            Some(landmark)
        }
        (Some(landmark), None) => Some(landmark),
        (None, Some(pose)) => Some(pose),
        (None, None) => None,
    }
}

fn log_vision_request_error(label: &str, error: Retained<NSError>) {
    let previous = VISION_FAILURES.fetch_add(1, Ordering::Relaxed);
    if previous == 0 {
        eprintln!(
            "Camera Vision {label} request failed: {}",
            crate::apple_platform::ns_error_description(error)
        );
    }
}

fn first_sample_from_results(
    results: &NSArray<VNFaceObservation>,
) -> (u64, Option<CameraMotionSample>) {
    let face_count = results.count();
    let sample = (face_count > 0)
        .then(|| results.objectAtIndex(0))
        .and_then(|face| sample_from_face_observation(face.deref()));
    (face_count as u64, sample)
}

fn latest_camera_sample() -> Option<CameraMotionSample> {
    LATEST_CAMERA_SAMPLE.lock().ok().and_then(|sample| *sample)
}

fn set_latest_camera_sample(sample: Option<CameraMotionSample>) {
    if let Ok(mut latest) = LATEST_CAMERA_SAMPLE.lock() {
        *latest = sample;
    }
}

fn sample_from_face_observation(face: &VNFaceObservation) -> Option<CameraMotionSample> {
    let bounding_box = unsafe { face.boundingBox() };
    if bounding_box.size.width <= 0.0 || bounding_box.size.height <= 0.0 {
        return None;
    }

    let center_x = (bounding_box.origin.x + bounding_box.size.width * 0.5) as f32;
    let center_y = (bounding_box.origin.y + bounding_box.size.height * 0.5) as f32;
    let mut sample = CameraMotionSample {
        face_offset: [
            ((center_x - 0.5) * 2.0).clamp(-1.0, 1.0),
            ((center_y - 0.5) * 2.0).clamp(-1.0, 1.0),
        ],
        face_angle: face_angle_from_observation(face),
        face_roll: normalized_face_roll(face),
        gaze: None,
        mouth_open: None,
        eye_open: None,
    };

    let Some(landmarks) = (unsafe { face.landmarks() }) else {
        return Some(sample);
    };

    let left_eye = unsafe { landmarks.leftEye() };
    let right_eye = unsafe { landmarks.rightEye() };
    let left_pupil = unsafe { landmarks.leftPupil() };
    let right_pupil = unsafe { landmarks.rightPupil() };
    let nose = unsafe { landmarks.nose() };
    let outer_lips = unsafe { landmarks.outerLips() };

    if sample.face_angle.is_none() {
        sample.face_angle = face_angle_from_landmarks(
            left_eye.as_deref(),
            right_eye.as_deref(),
            nose.as_deref(),
            outer_lips.as_deref(),
            sample.face_offset[1],
        );
    }

    sample.gaze = gaze_from_landmarks(
        left_eye.as_deref(),
        right_eye.as_deref(),
        left_pupil.as_deref(),
        right_pupil.as_deref(),
    );
    sample.eye_open = eye_open_from_landmarks(left_eye.as_deref(), right_eye.as_deref());
    sample.mouth_open = unsafe { landmarks.innerLips() }
        .as_deref()
        .and_then(mouth_open_from_lips)
        .or_else(|| outer_lips.as_deref().and_then(mouth_open_from_lips));

    Some(sample)
}

fn face_angle_from_observation(face: &VNFaceObservation) -> Option<[f32; 2]> {
    let yaw = unsafe { face.yaw() }.map(|yaw| normalize_face_yaw(yaw.as_f32()));
    let pitch = unsafe { face.pitch() }.map(|pitch| normalize_face_pitch(pitch.as_f32()));

    match (yaw, pitch) {
        (Some(yaw), Some(pitch)) => Some([yaw, pitch]),
        _ => None,
    }
}

fn normalized_face_roll(face: &VNFaceObservation) -> f32 {
    unsafe { face.roll() }
        .map(|roll| normalize_face_roll(roll.as_f32()))
        .unwrap_or(0.0)
}

fn normalize_face_yaw(radians: f32) -> f32 {
    (radians / FACE_YAW_NORMALIZATION_RADIANS).clamp(-1.0, 1.0)
}

fn normalize_face_pitch(radians: f32) -> f32 {
    (-radians / FACE_PITCH_NORMALIZATION_RADIANS).clamp(-1.0, 1.0)
}

fn normalize_face_roll(radians: f32) -> f32 {
    (radians / FACE_ROLL_NORMALIZATION_RADIANS).clamp(-1.0, 1.0)
}

fn face_angle_from_landmarks(
    left_eye: Option<&VNFaceLandmarkRegion2D>,
    right_eye: Option<&VNFaceLandmarkRegion2D>,
    nose: Option<&VNFaceLandmarkRegion2D>,
    mouth: Option<&VNFaceLandmarkRegion2D>,
    fallback_pitch: f32,
) -> Option<[f32; 2]> {
    let left_eye_center = left_eye.and_then(landmark_center)?;
    let right_eye_center = right_eye.and_then(landmark_center)?;
    let nose_center = nose.and_then(landmark_center)?;
    let mouth_center = mouth.and_then(landmark_center);

    Some(face_angle_from_landmark_geometry(
        left_eye_center,
        right_eye_center,
        nose_center,
        mouth_center,
        fallback_pitch,
    ))
}

fn face_angle_from_landmark_geometry(
    left_eye_center: [f32; 2],
    right_eye_center: [f32; 2],
    nose_center: [f32; 2],
    mouth_center: Option<[f32; 2]>,
    fallback_pitch: f32,
) -> [f32; 2] {
    let eye_mid = [
        (left_eye_center[0] + right_eye_center[0]) * 0.5,
        (left_eye_center[1] + right_eye_center[1]) * 0.5,
    ];
    let eye_distance = (right_eye_center[0] - left_eye_center[0]).abs().max(0.001);
    let yaw = ((nose_center[0] - eye_mid[0]) / (eye_distance * 0.32)).clamp(-1.0, 1.0);
    let pitch = mouth_center
        .map(|mouth_center| {
            let eye_to_mouth = (eye_mid[1] - mouth_center[1]).abs().max(0.001);
            let neutral_nose_ratio = 0.42;
            let nose_ratio = (eye_mid[1] - nose_center[1]) / eye_to_mouth;
            ((neutral_nose_ratio - nose_ratio) / 0.24).clamp(-1.0, 1.0)
        })
        .unwrap_or(fallback_pitch.clamp(-1.0, 1.0));

    [yaw, pitch]
}

fn gaze_from_landmarks(
    left_eye: Option<&VNFaceLandmarkRegion2D>,
    right_eye: Option<&VNFaceLandmarkRegion2D>,
    left_pupil: Option<&VNFaceLandmarkRegion2D>,
    right_pupil: Option<&VNFaceLandmarkRegion2D>,
) -> Option<[f32; 2]> {
    let mut samples = Vec::new();
    if let (Some(eye), Some(pupil)) = (left_eye, left_pupil) {
        samples.push(pupil_offset_in_eye(eye, pupil));
    }
    if let (Some(eye), Some(pupil)) = (right_eye, right_pupil) {
        samples.push(pupil_offset_in_eye(eye, pupil));
    }

    let mut sum = [0.0_f32, 0.0_f32];
    let mut count = 0.0_f32;
    for sample in samples.into_iter().flatten() {
        sum[0] += sample[0];
        sum[1] += sample[1];
        count += 1.0;
    }

    (count > 0.0).then(|| {
        [
            (sum[0] / count).clamp(-1.0, 1.0),
            (sum[1] / count).clamp(-1.0, 1.0),
        ]
    })
}

fn pupil_offset_in_eye(
    eye: &VNFaceLandmarkRegion2D,
    pupil: &VNFaceLandmarkRegion2D,
) -> Option<[f32; 2]> {
    let eye_bounds = landmark_bounds(eye)?;
    let pupil_center = landmark_center(pupil)?;
    let eye_center = [
        (eye_bounds.min[0] + eye_bounds.max[0]) * 0.5,
        (eye_bounds.min[1] + eye_bounds.max[1]) * 0.5,
    ];
    let eye_width = (eye_bounds.max[0] - eye_bounds.min[0]).max(0.001);
    let eye_height = (eye_bounds.max[1] - eye_bounds.min[1]).max(0.001);

    Some(pupil_offset_from_eye_geometry(
        eye_center,
        [eye_width, eye_height],
        pupil_center,
    ))
}

fn eye_open_from_landmarks(
    left_eye: Option<&VNFaceLandmarkRegion2D>,
    right_eye: Option<&VNFaceLandmarkRegion2D>,
) -> Option<f32> {
    let mut total = 0.0_f32;
    let mut count = 0.0_f32;
    for value in [left_eye, right_eye]
        .into_iter()
        .flatten()
        .filter_map(eye_open_ratio)
    {
        total += value;
        count += 1.0;
    }

    (count > 0.0).then(|| (total / count).clamp(0.0, 1.0))
}

fn eye_open_ratio(eye: &VNFaceLandmarkRegion2D) -> Option<f32> {
    let bounds = landmark_bounds(eye)?;
    let width = bounds.max[0] - bounds.min[0];
    let height = bounds.max[1] - bounds.min[1];
    if width <= 0.001 {
        return None;
    }

    Some(eye_open_from_geometry(width, height))
}

fn mouth_open_from_lips(lips: &VNFaceLandmarkRegion2D) -> Option<f32> {
    let bounds = landmark_bounds(lips)?;
    let width = bounds.max[0] - bounds.min[0];
    let height = bounds.max[1] - bounds.min[1];
    if width <= 0.001 {
        return None;
    }

    Some(mouth_open_from_geometry(width, height))
}

fn pupil_offset_from_eye_geometry(
    eye_center: [f32; 2],
    eye_size: [f32; 2],
    pupil_center: [f32; 2],
) -> [f32; 2] {
    [
        ((pupil_center[0] - eye_center[0]) / (eye_size[0].max(0.001) * 0.35)).clamp(-1.0, 1.0),
        ((pupil_center[1] - eye_center[1]) / (eye_size[1].max(0.001) * 0.50)).clamp(-1.0, 1.0),
    ]
}

fn eye_open_from_geometry(width: f32, height: f32) -> f32 {
    ((height / width.max(0.001)) / 0.45).clamp(0.0, 1.0)
}

fn mouth_open_from_geometry(width: f32, height: f32) -> f32 {
    ((height / width.max(0.001) - 0.08) / 0.42).clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LandmarkBounds {
    min: [f32; 2],
    max: [f32; 2],
}

fn landmark_center(region: &VNFaceLandmarkRegion2D) -> Option<[f32; 2]> {
    let points = landmark_points(region)?;
    let mut sum = [0.0_f32, 0.0_f32];
    for point in &points {
        sum[0] += point[0];
        sum[1] += point[1];
    }
    let count = points.len() as f32;
    Some([sum[0] / count, sum[1] / count])
}

fn landmark_bounds(region: &VNFaceLandmarkRegion2D) -> Option<LandmarkBounds> {
    let points = landmark_points(region)?;
    let mut bounds = LandmarkBounds {
        min: [f32::INFINITY, f32::INFINITY],
        max: [f32::NEG_INFINITY, f32::NEG_INFINITY],
    };

    for point in points {
        bounds.min[0] = bounds.min[0].min(point[0]);
        bounds.min[1] = bounds.min[1].min(point[1]);
        bounds.max[0] = bounds.max[0].max(point[0]);
        bounds.max[1] = bounds.max[1].max(point[1]);
    }

    Some(bounds)
}

fn landmark_points(region: &VNFaceLandmarkRegion2D) -> Option<Vec<[f32; 2]>> {
    let count = unsafe { region.pointCount() } as usize;
    if count == 0 {
        return None;
    }
    let raw_points = unsafe { region.normalizedPoints() };
    if raw_points.is_null() {
        return None;
    }

    let points = unsafe { std::slice::from_raw_parts(raw_points, count) };
    Some(
        points
            .iter()
            .map(|point| [point.x as f32, point.y as f32])
            .collect(),
    )
}

fn configure_capture_session(
    session: &AVCaptureSession,
    input: &AVCaptureDeviceInput,
    output: &AVCaptureVideoDataOutput,
) -> Result<(), String> {
    if !unsafe { session.canAddInput(input.deref()) } {
        return Err("AVCaptureSession rejected the camera input".to_string());
    }
    unsafe {
        session.addInput(input.deref());
    }

    if !unsafe { session.canAddOutput(output.deref()) } {
        return Err("AVCaptureSession rejected the video data output".to_string());
    }
    unsafe {
        session.addOutput(output.deref());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landmark_geometry_maps_mouth_open_to_normalized_range() {
        assert_eq!(mouth_open_from_geometry(1.0, 0.02), 0.0);
        assert!((mouth_open_from_geometry(1.0, 0.29) - 0.5).abs() < 0.001);
        assert_eq!(mouth_open_from_geometry(1.0, 0.80), 1.0);
    }

    #[test]
    fn landmark_geometry_maps_eye_open_to_normalized_range() {
        assert_eq!(eye_open_from_geometry(1.0, 0.0), 0.0);
        assert!((eye_open_from_geometry(1.0, 0.225) - 0.5).abs() < 0.001);
        assert_eq!(eye_open_from_geometry(1.0, 0.9), 1.0);
    }

    #[test]
    fn pupil_offset_geometry_is_centered_and_clamped() {
        assert_eq!(
            pupil_offset_from_eye_geometry([0.5, 0.5], [0.4, 0.2], [0.5, 0.5]),
            [0.0, 0.0]
        );
        assert_eq!(
            pupil_offset_from_eye_geometry([0.5, 0.5], [0.4, 0.2], [2.0, -2.0]),
            [1.0, -1.0]
        );
    }

    #[test]
    fn face_angles_map_typical_head_range_to_full_pose_range() {
        assert!((normalize_face_yaw(FRAC_PI_6) - 1.0).abs() < 0.001);
        assert!((normalize_face_yaw(-FRAC_PI_6) + 1.0).abs() < 0.001);
        assert!(
            (normalize_face_pitch(-(FACE_PITCH_NORMALIZATION_RADIANS * 0.5)) - 0.5).abs() < 0.001
        );
        assert!((normalize_face_roll(FACE_ROLL_NORMALIZATION_RADIANS * 0.5) - 0.5).abs() < 0.001);
    }

    #[test]
    fn landmark_geometry_estimates_head_yaw_when_native_pose_is_missing() {
        let centered = face_angle_from_landmark_geometry(
            [0.35, 0.70],
            [0.65, 0.70],
            [0.50, 0.53],
            Some([0.50, 0.30]),
            0.0,
        );
        let turned = face_angle_from_landmark_geometry(
            [0.35, 0.70],
            [0.65, 0.70],
            [0.58, 0.53],
            Some([0.50, 0.30]),
            0.0,
        );

        assert!(centered[0].abs() < 0.01);
        assert!(turned[0] > 0.5);
    }

    #[test]
    fn merge_pose_sample_keeps_landmarks_and_adds_rectangle_angles() {
        let landmark = CameraMotionSample {
            face_offset: [0.1, -0.2],
            face_angle: None,
            face_roll: 0.0,
            gaze: Some([0.3, -0.4]),
            mouth_open: Some(0.5),
            eye_open: Some(0.8),
        };
        let pose = CameraMotionSample {
            face_offset: [0.0, 0.0],
            face_angle: Some([0.6, -0.7]),
            face_roll: 0.25,
            gaze: None,
            mouth_open: None,
            eye_open: None,
        };

        let merged = merge_pose_sample(Some(landmark), Some(pose)).expect("merged sample");

        assert_eq!(merged.face_angle, Some([0.6, -0.7]));
        assert_eq!(merged.face_roll, 0.25);
        assert_eq!(merged.gaze, Some([0.3, -0.4]));
        assert_eq!(merged.mouth_open, Some(0.5));
        assert_eq!(merged.eye_open, Some(0.8));
    }
}
