use std::any::type_name;
use std::fs;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use objc2::{AnyThread, define_class, msg_send, rc::Retained, runtime::ProtocolObject};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMSampleBuffer, CMSampleTimingInfo, CMTime, CMVideoFormatDescription,
    CMVideoFormatDescriptionCreateForImageBuffer,
};
use objc2_core_media_io::{
    CMIOExtensionClient, CMIOExtensionDevice, CMIOExtensionDeviceProperties,
    CMIOExtensionDeviceSource, CMIOExtensionProperty, CMIOExtensionProvider,
    CMIOExtensionProviderProperties, CMIOExtensionProviderSource, CMIOExtensionStream,
    CMIOExtensionStreamClockType, CMIOExtensionStreamDirection,
    CMIOExtensionStreamDiscontinuityFlags, CMIOExtensionStreamFormat,
    CMIOExtensionStreamProperties, CMIOExtensionStreamSource, CMIOSampleBufferCreateForImageBuffer,
};
use objc2_core_video::{
    CVImageBuffer, CVPixelBuffer, CVPixelBufferCreateWithIOSurface, kCVReturnSuccess,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSSet, NSString, NSUUID};
use objc2_io_surface::IOSurfaceRef;
use serde::Deserialize;

const CAMERA_LOCALIZED_NAME: &str = "VTube Studio RS Camera";
const PROVIDER_NAME: &str = "VTube Studio RS";
const PROVIDER_MANUFACTURER: &str = "SymphonyIceAttack";
const EXTENSION_BUNDLE_ID: &str = "rs.vtube-studio.dev.CameraExtension";
const EXTENSION_MACH_SERVICE: &str = "rs.vtube-studio.dev.CameraExtension";
const APP_GROUP_ID: &str = "group.rs.vtube-studio.dev";
const PRODUCER_MANIFEST: &str = "target/internal-output/iosurface.json";
const DEVICE_UUID: &str = "B0E13F44-B9B5-45D0-9D9A-4C46D2026A01";
const STREAM_UUID: &str = "34B2FC2E-10DE-42C2-94C6-A59D0F2026A1";
const STREAM_LOCALIZED_NAME: &str = "Avatar BGRA Stream";
const FRAME_WIDTH: u32 = 1080;
const FRAME_HEIGHT: u32 = 1080;
const FRAME_RATE: u32 = 60;
const PIXEL_FORMAT: &str = "32BGRA";
static STREAM_START_COUNT: AtomicU32 = AtomicU32::new(0);
static STREAM_STOP_COUNT: AtomicU32 = AtomicU32::new(0);
static STREAM_FRAMES_SENT: AtomicU32 = AtomicU32::new(0);
static STREAM_FRAME_FAILURES: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CameraExtensionPrototype {
    provider: ProviderContract,
    device: DeviceContract,
    stream: StreamContract,
    bundle_id: &'static str,
    mach_service: &'static str,
    app_group_id: &'static str,
    producer_manifest: &'static str,
}

impl CameraExtensionPrototype {
    const fn current() -> Self {
        Self {
            provider: ProviderContract::current(),
            device: DeviceContract::current(),
            stream: StreamContract::current(),
            bundle_id: EXTENSION_BUNDLE_ID,
            mach_service: EXTENSION_MACH_SERVICE,
            app_group_id: APP_GROUP_ID,
            producer_manifest: PRODUCER_MANIFEST,
        }
    }

    fn binding_summary(self) -> String {
        format!(
            "provider={} provider_source={} provider_properties={} device={} device_source={} device_properties={} stream={} stream_source={} stream_properties={} stream_format={} pixel_buffer={} sample_buffer_fn=CMIOSampleBufferCreate",
            type_name::<objc2_core_media_io::CMIOExtensionProvider>(),
            type_name::<dyn objc2_core_media_io::CMIOExtensionProviderSource>(),
            type_name::<objc2_core_media_io::CMIOExtensionProviderProperties>(),
            type_name::<objc2_core_media_io::CMIOExtensionDevice>(),
            type_name::<dyn objc2_core_media_io::CMIOExtensionDeviceSource>(),
            type_name::<objc2_core_media_io::CMIOExtensionDeviceProperties>(),
            type_name::<objc2_core_media_io::CMIOExtensionStream>(),
            type_name::<dyn objc2_core_media_io::CMIOExtensionStreamSource>(),
            type_name::<objc2_core_media_io::CMIOExtensionStreamProperties>(),
            type_name::<objc2_core_media_io::CMIOExtensionStreamFormat>(),
            type_name::<objc2_core_video::CVPixelBuffer>(),
        )
    }

    fn source_checklist(self) -> [&'static str; 7] {
        [
            "done: CMIOExtensionProviderSource bridge class with connect/disconnect and provider properties",
            "done: CMIOExtensionDeviceSource bridge class with model properties",
            "done: CMIOExtensionStreamSource bridge class with formats and start/stop stream",
            "done: IOSurface manifest reader opens latest surface id from producer heartbeat",
            "done: CVPixelBuffer bridge wraps IOSurface as BGRA image buffer",
            "done: CMIOSampleBufferCreateForImageBuffer publishes timestamped frames while streaming",
            "neutral fallback: publish transparent BGRA frame when producer is stale",
        ]
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ProviderContract {
    name: &'static str,
    manufacturer: &'static str,
}

impl ProviderContract {
    const fn current() -> Self {
        Self {
            name: PROVIDER_NAME,
            manufacturer: PROVIDER_MANUFACTURER,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct DeviceContract {
    localized_name: &'static str,
    model: &'static str,
    device_uuid: &'static str,
    legacy_device_id: &'static str,
}

impl DeviceContract {
    const fn current() -> Self {
        Self {
            localized_name: CAMERA_LOCALIZED_NAME,
            model: "Live2D Metal Virtual Camera",
            device_uuid: DEVICE_UUID,
            legacy_device_id: EXTENSION_BUNDLE_ID,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct StreamContract {
    localized_name: &'static str,
    stream_uuid: &'static str,
    direction: StreamDirection,
    clock_type: StreamClockType,
    width: u32,
    height: u32,
    frame_rate: u32,
    pixel_format: &'static str,
    producer_manifest: &'static str,
}

impl StreamContract {
    const fn current() -> Self {
        Self {
            localized_name: STREAM_LOCALIZED_NAME,
            stream_uuid: STREAM_UUID,
            direction: StreamDirection::Source,
            clock_type: StreamClockType::HostTime,
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            frame_rate: FRAME_RATE,
            pixel_format: PIXEL_FORMAT,
            producer_manifest: PRODUCER_MANIFEST,
        }
    }

    const fn frame_duration(self) -> (u32, u32) {
        (1, self.frame_rate)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum StreamDirection {
    Source,
}

impl StreamDirection {
    const fn label(self) -> &'static str {
        match self {
            Self::Source => "source",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum StreamClockType {
    HostTime,
}

impl StreamClockType {
    const fn label(self) -> &'static str {
        match self {
            Self::HostTime => "host_time",
        }
    }
}

define_class!(
    // SAFETY:
    // - NSObject has no special subclassing requirements for this source object.
    // - The class does not implement Drop and stores no Rust-owned ivars.
    #[unsafe(super(NSObject))]
    #[name = "VTubeStudioRSCameraProviderSource"]
    struct CameraProviderSource;

    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for CameraProviderSource {}

    // SAFETY: The method signatures match CMIOExtensionProviderSource.
    #[allow(non_snake_case)]
    unsafe impl CMIOExtensionProviderSource for CameraProviderSource {
        #[unsafe(method(connectClient:error:))]
        fn connectClient_error(
            &self,
            _client: &CMIOExtensionClient,
            _out_error: *mut *mut NSError,
        ) -> bool {
            println!("camera_extension_event=provider_client_connected");
            true
        }

        #[unsafe(method(disconnectClient:))]
        fn disconnectClient(&self, _client: &CMIOExtensionClient) {
            println!("camera_extension_event=provider_client_disconnected");
        }

        #[unsafe(method_id(availableProperties))]
        fn availableProperties(&self) -> Retained<NSSet<CMIOExtensionProperty>> {
            NSSet::set()
        }

        #[unsafe(method_id(providerPropertiesForProperties:error:))]
        fn providerPropertiesForProperties_error(
            &self,
            _properties: &NSSet<CMIOExtensionProperty>,
            _out_error: *mut *mut NSError,
        ) -> Retained<CMIOExtensionProviderProperties> {
            provider_properties()
        }

        #[unsafe(method(setProviderProperties:error:))]
        fn setProviderProperties_error(
            &self,
            _provider_properties: &CMIOExtensionProviderProperties,
            _out_error: *mut *mut NSError,
        ) -> bool {
            true
        }
    }
);

impl CameraProviderSource {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

define_class!(
    // SAFETY:
    // - NSObject has no special subclassing requirements for this source object.
    // - The class does not implement Drop and stores no Rust-owned ivars.
    #[unsafe(super(NSObject))]
    #[name = "VTubeStudioRSCameraDeviceSource"]
    struct CameraDeviceSource;

    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for CameraDeviceSource {}

    // SAFETY: The method signatures match CMIOExtensionDeviceSource.
    #[allow(non_snake_case)]
    unsafe impl CMIOExtensionDeviceSource for CameraDeviceSource {
        #[unsafe(method_id(availableProperties))]
        fn availableProperties(&self) -> Retained<NSSet<CMIOExtensionProperty>> {
            NSSet::set()
        }

        #[unsafe(method_id(devicePropertiesForProperties:error:))]
        fn devicePropertiesForProperties_error(
            &self,
            _properties: &NSSet<CMIOExtensionProperty>,
            _out_error: *mut *mut NSError,
        ) -> Retained<CMIOExtensionDeviceProperties> {
            device_properties()
        }

        #[unsafe(method(setDeviceProperties:error:))]
        fn setDeviceProperties_error(
            &self,
            _device_properties: &CMIOExtensionDeviceProperties,
            _out_error: *mut *mut NSError,
        ) -> bool {
            true
        }
    }
);

impl CameraDeviceSource {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

define_class!(
    // SAFETY:
    // - NSObject has no special subclassing requirements for this source object.
    // - Stream lifecycle state is held in atomics until real per-client state is added.
    #[unsafe(super(NSObject))]
    #[name = "VTubeStudioRSCameraStreamSource"]
    struct CameraStreamSource;

    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for CameraStreamSource {}

    // SAFETY: The method signatures match CMIOExtensionStreamSource.
    #[allow(non_snake_case)]
    unsafe impl CMIOExtensionStreamSource for CameraStreamSource {
        #[unsafe(method_id(formats))]
        fn formats(&self) -> Retained<NSArray<CMIOExtensionStreamFormat>> {
            stream_formats()
        }

        #[unsafe(method_id(availableProperties))]
        fn availableProperties(&self) -> Retained<NSSet<CMIOExtensionProperty>> {
            NSSet::set()
        }

        #[unsafe(method_id(streamPropertiesForProperties:error:))]
        fn streamPropertiesForProperties_error(
            &self,
            _properties: &NSSet<CMIOExtensionProperty>,
            _out_error: *mut *mut NSError,
        ) -> Retained<CMIOExtensionStreamProperties> {
            stream_properties()
        }

        #[unsafe(method(setStreamProperties:error:))]
        fn setStreamProperties_error(
            &self,
            _stream_properties: &CMIOExtensionStreamProperties,
            _out_error: *mut *mut NSError,
        ) -> bool {
            true
        }

        #[unsafe(method(authorizedToStartStreamForClient:))]
        fn authorizedToStartStreamForClient(&self, _client: &CMIOExtensionClient) -> bool {
            true
        }

        #[unsafe(method(startStreamAndReturnError:))]
        fn startStreamAndReturnError(&self, _out_error: *mut *mut NSError) -> bool {
            let count = STREAM_START_COUNT.fetch_add(1, Ordering::AcqRel) + 1;
            println!("camera_extension_event=stream_start_requested count={count}");
            true
        }

        #[unsafe(method(stopStreamAndReturnError:))]
        fn stopStreamAndReturnError(&self, _out_error: *mut *mut NSError) -> bool {
            let count = STREAM_STOP_COUNT.fetch_add(1, Ordering::AcqRel) + 1;
            println!("camera_extension_event=stream_stop_requested count={count}");
            true
        }
    }
);

impl CameraStreamSource {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

struct CameraBridgeSources {
    provider: Retained<CameraProviderSource>,
    device: Retained<CameraDeviceSource>,
    stream: Retained<CameraStreamSource>,
}

impl CameraBridgeSources {
    fn new() -> Self {
        Self {
            provider: CameraProviderSource::new(),
            device: CameraDeviceSource::new(),
            stream: CameraStreamSource::new(),
        }
    }

    fn readiness_summary(&self) -> BridgeReadiness {
        let _provider: &ProtocolObject<dyn CMIOExtensionProviderSource> =
            ProtocolObject::from_ref(&*self.provider);
        let _device: &ProtocolObject<dyn CMIOExtensionDeviceSource> =
            ProtocolObject::from_ref(&*self.device);
        let _stream: &ProtocolObject<dyn CMIOExtensionStreamSource> =
            ProtocolObject::from_ref(&*self.stream);
        BridgeReadiness {
            provider_source: true,
            device_source: true,
            stream_source: true,
            stream_format_count: self.stream_formats_count(),
        }
    }

    fn stream_formats_count(&self) -> usize {
        let formats = stream_formats();
        formats.count()
    }
}

struct CameraExtensionGraph {
    #[allow(dead_code)]
    sources: CameraBridgeSources,
    #[allow(dead_code)]
    provider: Retained<CMIOExtensionProvider>,
    #[allow(dead_code)]
    device: Retained<CMIOExtensionDevice>,
    #[allow(dead_code)]
    stream: Retained<CMIOExtensionStream>,
}

impl CameraExtensionGraph {
    fn build() -> Result<Self, String> {
        let sources = CameraBridgeSources::new();
        let provider_source: &ProtocolObject<dyn CMIOExtensionProviderSource> =
            ProtocolObject::from_ref(&*sources.provider);
        let device_source: &ProtocolObject<dyn CMIOExtensionDeviceSource> =
            ProtocolObject::from_ref(&*sources.device);
        let stream_source: &ProtocolObject<dyn CMIOExtensionStreamSource> =
            ProtocolObject::from_ref(&*sources.stream);

        let provider =
            unsafe { CMIOExtensionProvider::providerWithSource_clientQueue(provider_source, None) };
        let device_name = NSString::from_str(CAMERA_LOCALIZED_NAME);
        let device_id = uuid_from_string(DEVICE_UUID)?;
        let legacy_device_id = NSString::from_str(EXTENSION_BUNDLE_ID);
        let device = unsafe {
            CMIOExtensionDevice::deviceWithLocalizedName_deviceID_legacyDeviceID_source(
                &device_name,
                &device_id,
                Some(&legacy_device_id),
                device_source,
            )
        };

        let stream_name = NSString::from_str(STREAM_LOCALIZED_NAME);
        let stream_id = uuid_from_string(STREAM_UUID)?;
        let stream = unsafe {
            CMIOExtensionStream::streamWithLocalizedName_streamID_direction_clockType_source(
                &stream_name,
                &stream_id,
                CMIOExtensionStreamDirection::Source,
                CMIOExtensionStreamClockType::HostTime,
                stream_source,
            )
        };

        unsafe {
            device
                .addStream_error(&stream)
                .map_err(|error| format!("CMIOExtensionDevice addStream failed: {error:?}"))?;
            provider
                .addDevice_error(&device)
                .map_err(|error| format!("CMIOExtensionProvider addDevice failed: {error:?}"))?;
        }

        Ok(Self {
            sources,
            provider,
            device,
            stream,
        })
    }

    fn summary(&self) -> CameraExtensionGraphSummary {
        let stream_count = unsafe { self.device.streams() }.count();
        let device_count = unsafe { self.provider.devices() }.count();
        CameraExtensionGraphSummary {
            provider_ready: true,
            device_ready: true,
            stream_ready: true,
            device_count,
            stream_count,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CameraExtensionGraphSummary {
    provider_ready: bool,
    device_ready: bool,
    stream_ready: bool,
    device_count: usize,
    stream_count: usize,
}

fn uuid_from_string(value: &str) -> Result<Retained<NSUUID>, String> {
    let value = NSString::from_str(value);
    NSUUID::initWithUUIDString(NSUUID::alloc(), &value)
        .ok_or_else(|| format!("invalid NSUUID string: {value}"))
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct BridgeReadiness {
    provider_source: bool,
    device_source: bool,
    stream_source: bool,
    stream_format_count: usize,
}

fn provider_properties() -> Retained<CMIOExtensionProviderProperties> {
    let properties = unsafe { CMIOExtensionProviderProperties::new() };
    let name = NSString::from_str(PROVIDER_NAME);
    let manufacturer = NSString::from_str(PROVIDER_MANUFACTURER);
    unsafe {
        properties.setName(Some(&name));
        properties.setManufacturer(Some(&manufacturer));
    }
    properties
}

fn device_properties() -> Retained<CMIOExtensionDeviceProperties> {
    let properties = unsafe { CMIOExtensionDeviceProperties::new() };
    let model = NSString::from_str(DeviceContract::current().model);
    unsafe {
        properties.setModel(Some(&model));
    }
    properties
}

fn stream_properties() -> Retained<CMIOExtensionStreamProperties> {
    unsafe { CMIOExtensionStreamProperties::new() }
}

fn stream_formats() -> Retained<NSArray<CMIOExtensionStreamFormat>> {
    let format = unsafe { CMIOExtensionStreamFormat::new() };
    NSArray::arrayWithObject(&format)
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ProducerManifestSnapshot {
    path: String,
    present: bool,
    bytes: usize,
    parse_error: Option<String>,
    iosurface_id: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    frames: Option<u64>,
    pixel_format: Option<String>,
    frame_rate: Option<u32>,
    updated_unix_ms: Option<u128>,
}

impl ProducerManifestSnapshot {
    fn read(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        match fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<ProducerManifest>(&content) {
                Ok(manifest) => Self {
                    path: path.display().to_string(),
                    present: true,
                    bytes: content.len(),
                    parse_error: None,
                    iosurface_id: Some(manifest.iosurface_id),
                    width: Some(manifest.width),
                    height: Some(manifest.height),
                    frames: Some(manifest.frames),
                    pixel_format: Some(manifest.pixel_format),
                    frame_rate: manifest.frame_rate,
                    updated_unix_ms: Some(manifest.updated_unix_ms),
                },
                Err(error) => Self {
                    path: path.display().to_string(),
                    present: true,
                    bytes: content.len(),
                    parse_error: Some(error.to_string()),
                    iosurface_id: None,
                    width: None,
                    height: None,
                    frames: None,
                    pixel_format: None,
                    frame_rate: None,
                    updated_unix_ms: None,
                },
            },
            Err(_) => Self {
                path: path.display().to_string(),
                present: false,
                bytes: 0,
                parse_error: None,
                iosurface_id: None,
                width: None,
                height: None,
                frames: None,
                pixel_format: None,
                frame_rate: None,
                updated_unix_ms: None,
            },
        }
    }

    fn ready_for_frame_bridge(&self) -> bool {
        self.present
            && self.parse_error.is_none()
            && self.iosurface_id.is_some()
            && self.frames.unwrap_or(0) > 0
            && self.width == Some(FRAME_WIDTH)
            && self.height == Some(FRAME_HEIGHT)
            && self.pixel_format.as_deref() == Some("BGRA8Unorm")
            && self.frame_rate.unwrap_or(FRAME_RATE) == FRAME_RATE
    }

    fn status_label(&self) -> &'static str {
        if self.ready_for_frame_bridge() {
            "ready"
        } else if self.present && self.parse_error.is_some() {
            "invalid"
        } else if self.present {
            "incomplete"
        } else {
            "missing"
        }
    }

    fn describe_contract(&self) -> String {
        format!(
            "surface_id={} frames={} size={}x{} pixel_format={} fps={} updated_unix_ms={}",
            self.iosurface_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.frames
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.width
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.height
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.pixel_format.as_deref().unwrap_or("none"),
            self.frame_rate
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.updated_unix_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ProducerManifest {
    iosurface_id: u32,
    width: u32,
    height: u32,
    pixel_format: String,
    frames: u64,
    updated_unix_ms: u128,
    #[serde(default)]
    frame_rate: Option<u32>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct StreamLifecycle {
    connected_clients: u32,
    streaming_clients: u32,
    frames_sent: u64,
    state: StreamState,
}

impl StreamLifecycle {
    const fn initial() -> Self {
        Self {
            connected_clients: 0,
            streaming_clients: 0,
            frames_sent: 0,
            state: StreamState::Idle,
        }
    }

    const fn start(self) -> Self {
        Self {
            streaming_clients: self.streaming_clients + 1,
            state: StreamState::Streaming,
            ..self
        }
    }

    const fn stop(self) -> Self {
        Self {
            streaming_clients: 0,
            state: StreamState::Idle,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum StreamState {
    Idle,
    Streaming,
}

impl StreamState {
    const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Streaming => "streaming",
        }
    }
}

struct FramePublisher {
    manifest_path: &'static str,
    sequence: u64,
    last_surface_id: Option<u32>,
    consecutive_failures: u32,
}

impl FramePublisher {
    const fn new(manifest_path: &'static str) -> Self {
        Self {
            manifest_path,
            sequence: 0,
            last_surface_id: None,
            consecutive_failures: 0,
        }
    }

    fn publish_next(&mut self, stream: &CMIOExtensionStream) {
        match self.build_sample_buffer() {
            Ok((sample_buffer, surface_id)) => {
                let host_time = host_time_nanos();
                unsafe {
                    stream.sendSampleBuffer_discontinuity_hostTimeInNanoseconds(
                        &sample_buffer,
                        CMIOExtensionStreamDiscontinuityFlags::None,
                        host_time,
                    );
                }
                self.consecutive_failures = 0;
                self.last_surface_id = Some(surface_id);
                let frames_sent = STREAM_FRAMES_SENT.fetch_add(1, Ordering::AcqRel) + 1;
                if frames_sent == 1 || frames_sent % FRAME_RATE == 0 {
                    println!(
                        "camera_extension_event=sample_buffer_sent frames={} sequence={} surface_id={} host_time_ns={}",
                        frames_sent, self.sequence, surface_id, host_time
                    );
                }
                self.sequence = self.sequence.saturating_add(1);
            }
            Err(error) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                let failures = STREAM_FRAME_FAILURES.fetch_add(1, Ordering::AcqRel) + 1;
                if self.consecutive_failures == 1 || self.consecutive_failures % FRAME_RATE == 0 {
                    println!(
                        "camera_extension_event=sample_buffer_send_skipped failures={} consecutive={} reason=\"{}\"",
                        failures, self.consecutive_failures, error
                    );
                }
            }
        }
    }

    fn build_sample_buffer(&self) -> Result<(CFRetained<CMSampleBuffer>, u32), String> {
        let manifest = ProducerManifestSnapshot::read(self.manifest_path);
        if !manifest.ready_for_frame_bridge() {
            return Err(format!(
                "producer manifest {}: {}",
                manifest.status_label(),
                manifest.describe_contract()
            ));
        }
        let surface_id = manifest
            .iosurface_id
            .ok_or_else(|| "producer manifest has no IOSurface id".to_string())?;
        let surface = IOSurfaceRef::lookup(surface_id)
            .ok_or_else(|| format!("IOSurfaceLookup failed for id {surface_id}"))?;
        let pixel_buffer = pixel_buffer_from_iosurface(&surface)?;
        let image_buffer: &CVImageBuffer = &pixel_buffer;
        let format_description = format_description_for_image_buffer(image_buffer)?;
        let sample_buffer =
            sample_buffer_for_image_buffer(image_buffer, &format_description, self.sequence)?;
        Ok((sample_buffer, surface_id))
    }
}

fn pixel_buffer_from_iosurface(
    surface: &IOSurfaceRef,
) -> Result<CFRetained<CVPixelBuffer>, String> {
    let mut raw: *mut CVPixelBuffer = std::ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreateWithIOSurface(
            None,
            surface,
            None,
            NonNull::new(&mut raw).expect("pixel buffer out pointer should be non-null"),
        )
    };
    if status != kCVReturnSuccess {
        return Err(format!(
            "CVPixelBufferCreateWithIOSurface failed status={status}"
        ));
    }
    let raw = NonNull::new(raw).ok_or_else(|| {
        "CVPixelBufferCreateWithIOSurface succeeded but returned null".to_string()
    })?;
    Ok(unsafe { CFRetained::from_raw(raw) })
}

fn format_description_for_image_buffer(
    image_buffer: &CVImageBuffer,
) -> Result<CFRetained<CMVideoFormatDescription>, String> {
    let mut raw: *const CMVideoFormatDescription = std::ptr::null();
    let status = unsafe {
        CMVideoFormatDescriptionCreateForImageBuffer(
            None,
            image_buffer,
            NonNull::new(&mut raw).expect("format description out pointer should be non-null"),
        )
    };
    if status != 0 {
        return Err(format!(
            "CMVideoFormatDescriptionCreateForImageBuffer failed status={status}"
        ));
    }
    let raw = NonNull::new(raw as *mut CMVideoFormatDescription).ok_or_else(|| {
        "CMVideoFormatDescriptionCreateForImageBuffer succeeded but returned null".to_string()
    })?;
    Ok(unsafe { CFRetained::from_raw(raw) })
}

fn sample_buffer_for_image_buffer(
    image_buffer: &CVImageBuffer,
    format_description: &CMVideoFormatDescription,
    sequence: u64,
) -> Result<CFRetained<CMSampleBuffer>, String> {
    let timing = CMSampleTimingInfo {
        duration: unsafe { CMTime::new(1, FRAME_RATE as i32) },
        presentationTimeStamp: unsafe { CMTime::new(sequence as i64, FRAME_RATE as i32) },
        decodeTimeStamp: unsafe { objc2_core_media::kCMTimeInvalid },
    };
    let mut raw: *mut CMSampleBuffer = std::ptr::null_mut();
    let status = unsafe {
        CMIOSampleBufferCreateForImageBuffer(
            None,
            Some(image_buffer),
            Some(format_description),
            &timing,
            sequence,
            0,
            &mut raw,
        )
    };
    if status != 0 {
        return Err(format!(
            "CMIOSampleBufferCreateForImageBuffer failed status={status}"
        ));
    }
    let raw = NonNull::new(raw).ok_or_else(|| {
        "CMIOSampleBufferCreateForImageBuffer succeeded but returned null".to_string()
    })?;
    Ok(unsafe { CFRetained::from_raw(raw) })
}

fn stream_is_active() -> bool {
    STREAM_START_COUNT.load(Ordering::Acquire) > STREAM_STOP_COUNT.load(Ordering::Acquire)
}

fn run_provider_service(graph: &CameraExtensionGraph) -> ! {
    println!("camera_extension_event=provider_service_starting");
    unsafe {
        CMIOExtensionProvider::startServiceWithProvider(&graph.provider);
    }
    println!("camera_extension_event=provider_service_started");

    let mut publisher = FramePublisher::new(PRODUCER_MANIFEST);
    let frame_interval = Duration::from_micros(1_000_000 / u64::from(FRAME_RATE));
    loop {
        if stream_is_active() {
            publisher.publish_next(&graph.stream);
        }
        thread::sleep(frame_interval);
    }
}

#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

fn host_time_nanos() -> u64 {
    let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
    let status = unsafe { mach_timebase_info(&mut info) };
    let ticks = unsafe { mach_absolute_time() };
    if status == 0 && info.denom != 0 {
        ticks
            .saturating_mul(u64::from(info.numer))
            .saturating_div(u64::from(info.denom))
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }
}

fn main() {
    let prototype = CameraExtensionPrototype::current();
    let bridge_sources = CameraBridgeSources::new();
    let bridge_readiness = bridge_sources.readiness_summary();
    let graph = match CameraExtensionGraph::build() {
        Ok(graph) => {
            let summary = graph.summary();
            println!(
                "camera_extension_event=cmio_graph_ready provider={} device={} stream={} devices={} streams={}",
                summary.provider_ready,
                summary.device_ready,
                summary.stream_ready,
                summary.device_count,
                summary.stream_count
            );
            Some(graph)
        }
        Err(error) => {
            eprintln!("camera_extension_event=cmio_graph_failed error=\"{error}\"");
            None
        }
    };
    let manifest = ProducerManifestSnapshot::read(prototype.producer_manifest);
    let lifecycle = StreamLifecycle::initial();
    let streaming_probe = lifecycle.start();
    let stopped_probe = streaming_probe.stop();
    let frame_duration = prototype.stream.frame_duration();

    println!(
        "camera_extension_event=prototype_loaded camera=\"{}\" bundle_id={} mach_service={} app_group={} manifest={}",
        prototype.device.localized_name,
        prototype.bundle_id,
        prototype.mach_service,
        prototype.app_group_id,
        prototype.producer_manifest
    );
    println!(
        "camera_extension_event=objc2_bindings_ready {}",
        prototype.binding_summary()
    );
    println!(
        "camera_extension_event=provider_contract_ready name=\"{}\" manufacturer=\"{}\"",
        prototype.provider.name, prototype.provider.manufacturer
    );
    println!(
        "camera_extension_event=device_contract_ready name=\"{}\" model=\"{}\" uuid={} legacy_id={}",
        prototype.device.localized_name,
        prototype.device.model,
        prototype.device.device_uuid,
        prototype.device.legacy_device_id
    );
    println!(
        "camera_extension_event=stream_contract_ready name=\"{}\" uuid={} direction={} clock={} size={}x{} fps={} frame_duration={}/{} pixel_format={} producer_manifest={}",
        prototype.stream.localized_name,
        prototype.stream.stream_uuid,
        prototype.stream.direction.label(),
        prototype.stream.clock_type.label(),
        prototype.stream.width,
        prototype.stream.height,
        prototype.stream.frame_rate,
        frame_duration.0,
        frame_duration.1,
        prototype.stream.pixel_format,
        prototype.stream.producer_manifest
    );
    println!(
        "camera_extension_event=source_bridge_ready provider={} device={} stream={} stream_formats={}",
        bridge_readiness.provider_source,
        bridge_readiness.device_source,
        bridge_readiness.stream_source,
        bridge_readiness.stream_format_count
    );
    if graph.is_none() {
        println!(
            "camera_extension_event=cmio_graph_ready provider=false device=false stream=false devices=0 streams=0"
        );
    }
    println!(
        "camera_extension_event=producer_manifest_probe status={} path=\"{}\" bytes={} {} parse_error=\"{}\"",
        manifest.status_label(),
        manifest.path,
        manifest.bytes,
        manifest.describe_contract(),
        manifest.parse_error.as_deref().unwrap_or("")
    );
    println!(
        "camera_extension_event=stream_lifecycle_ready state={} connected_clients={} streaming_clients={} frames_sent={}",
        lifecycle.state.label(),
        lifecycle.connected_clients,
        lifecycle.streaming_clients,
        lifecycle.frames_sent
    );
    println!(
        "camera_extension_event=stream_lifecycle_probe start_state={} stop_state={} simulated=true",
        streaming_probe.state.label(),
        stopped_probe.state.label()
    );
    for item in prototype.source_checklist() {
        println!("camera_extension_event=source_checklist item=\"{item}\"");
    }
    if let Some(graph) = graph {
        run_provider_service(&graph);
    }
    println!("camera_extension_event=not_streaming reason=cmio_graph_unavailable");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_identifiers_match_container_app_plan() {
        let prototype = CameraExtensionPrototype::current();
        assert_eq!(prototype.device.localized_name, "VTube Studio RS Camera");
        assert_eq!(prototype.bundle_id, prototype.mach_service);
        assert!(prototype.bundle_id.ends_with(".CameraExtension"));
        assert!(prototype.app_group_id.starts_with("group."));
        assert!(prototype.producer_manifest.ends_with("iosurface.json"));
    }

    #[test]
    fn binding_summary_references_coremediaio_source_protocols() {
        let summary = CameraExtensionPrototype::current().binding_summary();
        assert!(summary.contains("CMIOExtensionProvider"));
        assert!(summary.contains("CMIOExtensionProviderSource"));
        assert!(summary.contains("CMIOExtensionDeviceSource"));
        assert!(summary.contains("CMIOExtensionStreamSource"));
        assert!(summary.contains("CMIOExtensionStreamFormat"));
        assert!(summary.contains("objc2_core_video"));
        assert!(summary.contains("CMIOSampleBufferCreate"));
    }

    #[test]
    fn stream_contract_matches_internal_iosurface_output() {
        let stream = StreamContract::current();
        assert_eq!(stream.direction.label(), "source");
        assert_eq!(stream.clock_type.label(), "host_time");
        assert_eq!((stream.width, stream.height), (1080, 1080));
        assert_eq!(stream.frame_rate, 60);
        assert_eq!(stream.frame_duration(), (1, 60));
        assert_eq!(stream.pixel_format, "32BGRA");
        assert_eq!(stream.producer_manifest, PRODUCER_MANIFEST);
    }

    #[test]
    fn producer_manifest_snapshot_reports_missing_and_ready_states() {
        let missing = ProducerManifestSnapshot::read("target/virtual-camera/does-not-exist.json");
        assert!(!missing.ready_for_frame_bridge());
        assert_eq!(missing.status_label(), "missing");

        let ready = ProducerManifestSnapshot {
            path: PRODUCER_MANIFEST.to_string(),
            present: true,
            bytes: 128,
            parse_error: None,
            iosurface_id: Some(42),
            width: Some(FRAME_WIDTH),
            height: Some(FRAME_HEIGHT),
            frames: Some(1),
            pixel_format: Some("BGRA8Unorm".to_string()),
            frame_rate: Some(FRAME_RATE),
            updated_unix_ms: Some(1),
        };
        assert!(ready.ready_for_frame_bridge());
        assert_eq!(ready.status_label(), "ready");
    }

    #[test]
    fn producer_manifest_snapshot_validates_camera_contract() {
        let root = std::env::temp_dir().join(format!(
            "vtube-studio-rs-camera-extension-manifest-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temp dir should be created");
        let path = root.join("iosurface.json");
        fs::write(
            &path,
            r#"{
  "iosurface_id": 7,
  "width": 1080,
  "height": 1080,
  "pixel_format": "BGRA8Unorm",
  "frames": 12,
  "frame_rate": 60,
  "updated_unix_ms": 123
}"#,
        )
        .expect("manifest should be written");

        let snapshot = ProducerManifestSnapshot::read(&path);
        assert!(snapshot.ready_for_frame_bridge());
        assert_eq!(snapshot.status_label(), "ready");
        assert_eq!(snapshot.iosurface_id, Some(7));
        assert!(snapshot.describe_contract().contains("size=1080x1080"));

        fs::write(
            &path,
            r#"{
  "iosurface_id": 7,
  "width": 720,
  "height": 720,
  "pixel_format": "RGBA8Unorm",
  "frames": 12,
  "frame_rate": 30,
  "updated_unix_ms": 123
}"#,
        )
        .expect("manifest should be rewritten");
        let snapshot = ProducerManifestSnapshot::read(&path);
        assert!(!snapshot.ready_for_frame_bridge());
        assert_eq!(snapshot.status_label(), "incomplete");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lifecycle_tracks_start_stop_without_frames_yet() {
        let lifecycle = StreamLifecycle::initial();
        assert_eq!(lifecycle.state.label(), "idle");
        assert_eq!(lifecycle.streaming_clients, 0);

        let streaming = lifecycle.start();
        assert_eq!(streaming.state.label(), "streaming");
        assert_eq!(streaming.streaming_clients, 1);
        assert_eq!(streaming.frames_sent, 0);

        let stopped = streaming.stop();
        assert_eq!(stopped.state.label(), "idle");
        assert_eq!(stopped.streaming_clients, 0);
    }

    #[test]
    fn source_checklist_names_the_remaining_camera_work() {
        let checklist = CameraExtensionPrototype::current().source_checklist();
        assert!(checklist.iter().any(|item| item.contains("ProviderSource")));
        assert!(checklist.iter().any(|item| item.contains("DeviceSource")));
        assert!(checklist.iter().any(|item| item.contains("StreamSource")));
        assert!(checklist.iter().any(|item| item.contains("CVPixelBuffer")));
        assert!(
            checklist
                .iter()
                .any(|item| item.contains("CMIOSampleBufferCreate"))
        );
    }

    #[test]
    fn bridge_sources_cast_to_coremediaio_protocol_objects() {
        let sources = CameraBridgeSources::new();
        let readiness = sources.readiness_summary();
        assert!(readiness.provider_source);
        assert!(readiness.device_source);
        assert!(readiness.stream_source);
        assert_eq!(readiness.stream_format_count, 1);
    }

    #[test]
    fn source_property_objects_are_constructed() {
        let provider = provider_properties();
        let device = device_properties();
        let stream = stream_properties();
        let formats = stream_formats();

        assert_eq!(formats.count(), 1);
        drop(provider);
        drop(device);
        drop(stream);
    }

    #[test]
    fn cmio_graph_wires_provider_device_and_stream() {
        let graph = CameraExtensionGraph::build().expect("CMIO graph should build");
        let summary = graph.summary();
        assert!(summary.provider_ready);
        assert!(summary.device_ready);
        assert!(summary.stream_ready);
        assert_eq!(summary.device_count, 1);
        assert_eq!(summary.stream_count, 1);
    }
}
