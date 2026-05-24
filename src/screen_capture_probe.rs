#![cfg(target_os = "macos")]

use crate::config::{RuntimeProfile, ScreenCaptureKitConfig};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[allow(dead_code)]
pub enum ScreenCaptureProbeStatus {
    Disabled,
    Unsupported,
    WaitingPermission,
    PermissionDenied,
    WindowNotFound,
    Starting,
    Running,
    Stalled,
    Failed,
}

impl ScreenCaptureProbeStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::WaitingPermission => "waiting permission",
            Self::PermissionDenied => "permission denied",
            Self::WindowNotFound => "window not found",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stalled => "stalled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScreenCaptureProbeSnapshot {
    pub status: ScreenCaptureProbeStatus,
    pub frames: u64,
    pub last_frame_age_seconds: Option<f64>,
    pub diagnostic: Option<String>,
}

pub struct ScreenCaptureProbe {
    runtime: Option<backend::ScreenCaptureRuntime>,
    status: ScreenCaptureProbeStatus,
    diagnostic: Option<String>,
    log_interval: Duration,
    stalled_after: Duration,
    last_log_at: Instant,
    last_reported_frames: u64,
    stalled_logged: bool,
}

impl ScreenCaptureProbe {
    pub fn disabled_with_reason(
        config: &ScreenCaptureKitConfig,
        reason: impl Into<String>,
    ) -> Self {
        Self::static_status(
            ScreenCaptureProbeStatus::Disabled,
            Some(reason.into()),
            config,
        )
    }

    pub fn start(
        window: *mut std::ffi::c_void,
        config: &ScreenCaptureKitConfig,
        profile: RuntimeProfile,
    ) -> Self {
        if !config.enabled(profile) {
            return Self::static_status(ScreenCaptureProbeStatus::Disabled, None, config);
        }

        let window_number = unsafe { crate::apple_platform::panel_window_number(window) };
        if window_number <= 0 {
            return Self::static_status(
                ScreenCaptureProbeStatus::Failed,
                Some("Avatar window did not have a valid NSWindow number.".to_string()),
                config,
            );
        }

        match backend::ScreenCaptureRuntime::start(window_number as u32, config) {
            Ok(runtime) => {
                println!(
                    "renderer_event=sckit_probe_started window_id={} target_fps={} log_interval_s={:.1} stalled_after_s={:.1}",
                    window_number,
                    config.target_fps.clamp(1, 60),
                    valid_seconds(config.log_interval_seconds, 2.0),
                    valid_seconds(config.stalled_after_seconds, 2.0)
                );
                Self {
                    runtime: Some(runtime),
                    status: ScreenCaptureProbeStatus::Starting,
                    diagnostic: None,
                    log_interval: seconds_duration(config.log_interval_seconds, 2.0),
                    stalled_after: seconds_duration(config.stalled_after_seconds, 2.0),
                    last_log_at: Instant::now(),
                    last_reported_frames: 0,
                    stalled_logged: false,
                }
            }
            Err(error) => {
                println!(
                    "renderer_event=sckit_probe_failed status={} detail={:?}",
                    error.status.label(),
                    error.diagnostic
                );
                Self::static_status(error.status, Some(error.diagnostic), config)
            }
        }
    }

    fn static_status(
        status: ScreenCaptureProbeStatus,
        diagnostic: Option<String>,
        config: &ScreenCaptureKitConfig,
    ) -> Self {
        Self {
            runtime: None,
            status,
            diagnostic,
            log_interval: seconds_duration(config.log_interval_seconds, 2.0),
            stalled_after: seconds_duration(config.stalled_after_seconds, 2.0),
            last_log_at: Instant::now(),
            last_reported_frames: 0,
            stalled_logged: false,
        }
    }

    pub fn poll(&mut self) -> ScreenCaptureProbeSnapshot {
        let Some(runtime) = self.runtime.as_ref() else {
            return self.snapshot();
        };

        let frames = runtime.frame_count();
        let last_frame_age = runtime.last_frame_age();
        let previous_status = self.status;
        self.status = if frames == 0 {
            ScreenCaptureProbeStatus::Starting
        } else if last_frame_age.is_some_and(|age| age > self.stalled_after) {
            ScreenCaptureProbeStatus::Stalled
        } else {
            ScreenCaptureProbeStatus::Running
        };

        if self.status == ScreenCaptureProbeStatus::Stalled && !self.stalled_logged {
            println!(
                "renderer_event=sckit_stalled frames={} last_frame_age_s={:.2}",
                frames,
                last_frame_age.map(|age| age.as_secs_f64()).unwrap_or(-1.0)
            );
            self.stalled_logged = true;
        } else if previous_status == ScreenCaptureProbeStatus::Stalled
            && self.status == ScreenCaptureProbeStatus::Running
        {
            println!(
                "renderer_event=sckit_recovered frames={} last_frame_age_s={:.2}",
                frames,
                last_frame_age.map(|age| age.as_secs_f64()).unwrap_or(0.0)
            );
            self.stalled_logged = false;
        }

        let now = Instant::now();
        if now.duration_since(self.last_log_at) >= self.log_interval {
            let delta = frames.saturating_sub(self.last_reported_frames);
            println!(
                "renderer_event=sckit_frame_summary status={} frames={} delta={} last_frame_age_s={:.2}",
                self.status.label(),
                frames,
                delta,
                last_frame_age.map(|age| age.as_secs_f64()).unwrap_or(-1.0)
            );
            self.last_log_at = now;
            self.last_reported_frames = frames;
        }

        self.snapshot()
    }

    fn snapshot(&self) -> ScreenCaptureProbeSnapshot {
        let frames = self
            .runtime
            .as_ref()
            .map(backend::ScreenCaptureRuntime::frame_count)
            .unwrap_or(0);
        let last_frame_age_seconds = self
            .runtime
            .as_ref()
            .and_then(backend::ScreenCaptureRuntime::last_frame_age)
            .map(|age| age.as_secs_f64());
        ScreenCaptureProbeSnapshot {
            status: self.status,
            frames,
            last_frame_age_seconds,
            diagnostic: self.diagnostic.clone(),
        }
    }
}

impl Drop for ScreenCaptureProbe {
    fn drop(&mut self) {
        if self.runtime.is_some() {
            println!("renderer_event=sckit_probe_stopped");
        }
    }
}

pub fn overlay_text(snapshot: &ScreenCaptureProbeSnapshot) -> String {
    let age = snapshot
        .last_frame_age_seconds
        .map(|age| format!("{age:.1}s"))
        .unwrap_or_else(|| "none".to_string());
    let mut text = format!(
        "ScreenCaptureKit: {} | frames {} | last {}",
        snapshot.status.label(),
        snapshot.frames,
        age
    );
    if let Some(diagnostic) = snapshot
        .diagnostic
        .as_deref()
        .and_then(first_non_empty_line)
    {
        text.push_str("\nScreenCaptureKit detail: ");
        text.push_str(diagnostic);
    }
    text
}

pub fn menu_text(snapshot: &ScreenCaptureProbeSnapshot) -> String {
    format!(
        "ScreenCaptureKit Probe: {} | frames {}",
        snapshot.status.label(),
        snapshot.frames
    )
}

fn first_non_empty_line(value: &str) -> Option<&str> {
    value.lines().map(str::trim).find(|line| !line.is_empty())
}

fn seconds_duration(value: f32, fallback: f32) -> Duration {
    Duration::from_secs_f32(valid_seconds(value, fallback))
}

fn valid_seconds(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

#[cfg(feature = "screen-capture-kit")]
mod backend {
    use super::ScreenCaptureProbeStatus;
    use crate::config::ScreenCaptureKitConfig;
    use block2::RcBlock;
    use dispatch2::{DispatchQueue, DispatchRetained};
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, extern_methods};
    use objc2_core_media::{CMSampleBuffer, CMTime};
    use objc2_foundation::NSError;
    use objc2_screen_capture_kit::{
        SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
        SCStreamOutput, SCStreamOutputType, SCWindow,
    };
    use std::ops::Deref;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);
    static LAST_FRAME_NS: AtomicU64 = AtomicU64::new(0);
    static STREAM_STOP_ERRORS: AtomicU64 = AtomicU64::new(0);

    const PIXEL_FORMAT_BGRA: u32 = u32::from_be_bytes(*b"BGRA");

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "VtubeStudioRsScreenCaptureProbeOutput"]
        struct ScreenCaptureProbeOutput;

        unsafe impl NSObjectProtocol for ScreenCaptureProbeOutput {}

        unsafe impl SCStreamOutput for ScreenCaptureProbeOutput {}

        unsafe impl SCStreamDelegate for ScreenCaptureProbeOutput {}

        impl ScreenCaptureProbeOutput {
            #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
            fn stream_did_output_sample_buffer(
                &self,
                _stream: &SCStream,
                _sample_buffer: &CMSampleBuffer,
                output_type: SCStreamOutputType,
            ) {
                if output_type == SCStreamOutputType::Screen {
                    FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
                    LAST_FRAME_NS.store(monotonicish_time_ns(), Ordering::Relaxed);
                }
            }

            #[unsafe(method(stream:didStopWithError:))]
            fn stream_did_stop_with_error(&self, _stream: &SCStream, error: &NSError) {
                STREAM_STOP_ERRORS.fetch_add(1, Ordering::Relaxed);
                println!(
                    "renderer_event=sckit_probe_failed status=failed detail={:?}",
                    crate::apple_platform::foundation_string(&error.localizedDescription())
                );
            }
        }
    );

    impl ScreenCaptureProbeOutput {
        extern_methods!(
            #[unsafe(method(new))]
            fn new() -> Retained<Self>;
        );
    }

    pub struct ScreenCaptureRuntime {
        window: Retained<SCWindow>,
        filter: Retained<SCContentFilter>,
        configuration: Retained<SCStreamConfiguration>,
        stream: Retained<SCStream>,
        output: Retained<ScreenCaptureProbeOutput>,
        queue: DispatchRetained<DispatchQueue>,
    }

    pub struct ScreenCaptureStartError {
        pub status: ScreenCaptureProbeStatus,
        pub diagnostic: String,
    }

    impl ScreenCaptureRuntime {
        pub fn start(
            window_id: u32,
            config: &ScreenCaptureKitConfig,
        ) -> Result<Self, ScreenCaptureStartError> {
            reset_counters();
            let window = find_shareable_window(window_id)?;
            let filter = unsafe {
                SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window)
            };
            let stream_config = stream_configuration(&window, config);
            let output = ScreenCaptureProbeOutput::new();
            let queue = DispatchQueue::new("rs.vtube-studio.screen-capture-probe", None);
            let stream = unsafe {
                SCStream::initWithFilter_configuration_delegate(
                    SCStream::alloc(),
                    &filter,
                    &stream_config,
                    Some(ProtocolObject::from_ref(output.deref())),
                )
            };
            unsafe {
                stream
                    .addStreamOutput_type_sampleHandlerQueue_error(
                        ProtocolObject::from_ref(output.deref()),
                        SCStreamOutputType::Screen,
                        Some(queue.deref()),
                    )
                    .map_err(|error| ScreenCaptureStartError {
                        status: ScreenCaptureProbeStatus::Failed,
                        diagnostic: local_screen_capture_detail(&format!(
                            "Failed to add ScreenCaptureKit stream output: {}",
                            crate::apple_platform::ns_error_description(error)
                        )),
                    })?;
            }
            wait_for_stream_start(&stream)?;

            Ok(Self {
                window,
                filter,
                configuration: stream_config,
                stream,
                output,
                queue,
            })
        }

        pub fn frame_count(&self) -> u64 {
            FRAME_COUNT.load(Ordering::Relaxed)
        }

        pub fn last_frame_age(&self) -> Option<Duration> {
            let last = LAST_FRAME_NS.load(Ordering::Relaxed);
            if last == 0 {
                None
            } else {
                Some(Duration::from_nanos(
                    monotonicish_time_ns().saturating_sub(last),
                ))
            }
        }
    }

    impl Drop for ScreenCaptureRuntime {
        fn drop(&mut self) {
            unsafe {
                self.stream.stopCaptureWithCompletionHandler(None);
            }
            let _ = (
                &self.window,
                &self.filter,
                &self.configuration,
                &self.output,
                &self.queue,
            );
        }
    }

    fn stream_configuration(
        window: &SCWindow,
        config: &ScreenCaptureKitConfig,
    ) -> Retained<SCStreamConfiguration> {
        let stream_config = unsafe { SCStreamConfiguration::new() };
        let frame = unsafe { window.frame() };
        let width = (frame.size.width.max(1.0) * 2.0).round() as usize;
        let height = (frame.size.height.max(1.0) * 2.0).round() as usize;
        unsafe {
            stream_config.setWidth(width.max(1));
            stream_config.setHeight(height.max(1));
            stream_config
                .setMinimumFrameInterval(CMTime::new(1, config.target_fps.clamp(1, 60) as i32));
            stream_config.setPixelFormat(PIXEL_FORMAT_BGRA);
            stream_config.setShowsCursor(false);
            stream_config.setQueueDepth(3);
            stream_config.setScalesToFit(false);
            stream_config.setPreservesAspectRatio(true);
        }
        stream_config
    }

    fn find_shareable_window(
        window_id: u32,
    ) -> Result<Retained<SCWindow>, ScreenCaptureStartError> {
        let (sender, receiver) = mpsc::channel::<Result<usize, ScreenCaptureStartError>>();
        let handler = RcBlock::new(
            move |content: *mut SCShareableContent, error: *mut NSError| {
                if !error.is_null() {
                    let error = unsafe { &*error };
                    let detail =
                        crate::apple_platform::foundation_string(&error.localizedDescription());
                    let _ = sender.send(Err(classify_content_error(&detail)));
                    return;
                }
                if content.is_null() {
                    let _ = sender.send(Err(ScreenCaptureStartError {
                        status: ScreenCaptureProbeStatus::Failed,
                        diagnostic: local_screen_capture_detail(
                            "ScreenCaptureKit returned no shareable content and no error.",
                        ),
                    }));
                    return;
                }

                let content = unsafe { &*content };
                let windows = unsafe { content.windows() };
                for index in 0..windows.len() {
                    let window = windows.objectAtIndex(index);
                    if unsafe { window.windowID() } == window_id {
                        let pointer = Retained::into_raw(window) as usize;
                        let _ = sender.send(Ok(pointer));
                        return;
                    }
                }

                let _ = sender.send(Err(ScreenCaptureStartError {
                status: ScreenCaptureProbeStatus::WindowNotFound,
                diagnostic: local_screen_capture_detail(&format!(
                    "ScreenCaptureKit could not find avatar window id {window_id}. Check Screen Recording permission and whether the window is visible."
                )),
            }));
            },
        );

        unsafe {
            SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
                true,
                false,
                &handler,
            );
        }

        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(pointer)) => unsafe {
                Retained::from_raw(pointer as *mut SCWindow).ok_or_else(|| {
                    ScreenCaptureStartError {
                        status: ScreenCaptureProbeStatus::Failed,
                        diagnostic: local_screen_capture_detail(
                            "ScreenCaptureKit returned a null SCWindow pointer.",
                        ),
                    }
                })
            },
            Ok(Err(error)) => Err(error),
            Err(error) => Err(ScreenCaptureStartError {
                status: ScreenCaptureProbeStatus::WaitingPermission,
                diagnostic: local_screen_capture_detail(&format!(
                    "Timed out while waiting for ScreenCaptureKit shareable content: {error}. Approve Screen Recording permission for vtube-studio-rs Dev, then restart."
                )),
            }),
        }
    }

    fn wait_for_stream_start(stream: &SCStream) -> Result<(), ScreenCaptureStartError> {
        let (sender, receiver) = mpsc::channel::<Result<(), String>>();
        let handler = RcBlock::new(move |error: *mut NSError| {
            if error.is_null() {
                let _ = sender.send(Ok(()));
            } else {
                let error = unsafe { &*error };
                let detail =
                    crate::apple_platform::foundation_string(&error.localizedDescription());
                let _ = sender.send(Err(detail));
            }
        });
        unsafe {
            stream.startCaptureWithCompletionHandler(Some(&handler));
        }
        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(classify_content_error(&error)),
            Err(error) => Err(ScreenCaptureStartError {
                status: ScreenCaptureProbeStatus::WaitingPermission,
                diagnostic: local_screen_capture_detail(&format!(
                    "Timed out while starting ScreenCaptureKit stream: {error}. Approve Screen Recording permission for vtube-studio-rs Dev, then restart."
                )),
            }),
        }
    }

    fn classify_content_error(detail: &str) -> ScreenCaptureStartError {
        let lower = detail.to_ascii_lowercase();
        let status = if lower.contains("denied")
            || lower.contains("permission")
            || lower.contains("privacy")
            || lower.contains("authorized")
        {
            ScreenCaptureProbeStatus::PermissionDenied
        } else {
            ScreenCaptureProbeStatus::Failed
        };
        ScreenCaptureStartError {
            status,
            diagnostic: local_screen_capture_detail(&format!(
                "ScreenCaptureKit setup failed: {detail}"
            )),
        }
    }

    fn local_screen_capture_detail(detail: &str) -> String {
        crate::apple_platform::local_only_screen_capture_message(detail)
    }

    fn reset_counters() {
        FRAME_COUNT.store(0, Ordering::Relaxed);
        LAST_FRAME_NS.store(0, Ordering::Relaxed);
        STREAM_STOP_ERRORS.store(0, Ordering::Relaxed);
    }

    fn monotonicish_time_ns() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
            .unwrap_or(0)
    }
}

#[cfg(not(feature = "screen-capture-kit"))]
mod backend {
    use super::ScreenCaptureProbeStatus;
    use crate::config::ScreenCaptureKitConfig;
    use std::time::Duration;

    pub struct ScreenCaptureRuntime;

    pub struct ScreenCaptureStartError {
        pub status: ScreenCaptureProbeStatus,
        pub diagnostic: String,
    }

    impl ScreenCaptureRuntime {
        pub fn start(
            _window_id: u32,
            _config: &ScreenCaptureKitConfig,
        ) -> Result<Self, ScreenCaptureStartError> {
            Err(ScreenCaptureStartError {
                status: ScreenCaptureProbeStatus::Unsupported,
                diagnostic: "Rebuild with the screen-capture-kit feature to enable the ScreenCaptureKit runtime probe.".to_string(),
            })
        }

        pub fn frame_count(&self) -> u64 {
            0
        }

        pub fn last_frame_age(&self) -> Option<Duration> {
            None
        }
    }
}
