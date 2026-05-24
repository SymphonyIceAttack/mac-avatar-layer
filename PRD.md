# vtube-studio-rs PRD

## Product Goal

Build a Rust-first macOS avatar host inspired by VTube Studio. The first
product advantage is reliable avatar rendering across macOS Desktop/Space
switches, while still loading real Live2D Cubism assets from local `public/`
model folders and rendering them through Cubism Core + Metal.

The project should grow toward a practical daily avatar host: stable windowing,
correct rendering, basic motion/expression/physics, simple input drivers, and a
local developer workflow that does not depend on checked-in SDK/model assets.

## MVP Definition

The current MVP is considered usable when all of the following work from local
files:

- Load a `.model3.json` model from `public/`, including `.moc3`, texture atlas,
  display info, motions, expressions, and physics when present.
- Evaluate the model through official Cubism Core via `live2d-cubism-core-sys`.
- Render drawable meshes through Metal with correct draw order, blend modes,
  clipping masks, texture atlas sampling, offscreen composites, and basic edge
  quality.
- Drive first-pass avatar motion from idle blink/breath, `.motion3.json`,
  `.exp3.json`, `.physics3.json`, mouse, microphone, and local TOML overrides.
- Keep the native transparent AppKit window visible and rendering across macOS
  Spaces and common display transitions.
- Provide one-command local run and regression tooling through `cargo xtask`.

## Completed Core Capabilities

### Runtime And Model Loading

- `public/` is intentionally local-only and ignored by Git.
- The app loads `public/model/0.model3.json` and other local `.model3.json`
  files, including `.moc3`, textures, physics, display info, motions, and
  expressions when present.
- Cubism Core is integrated through `live2d-cubism-core-sys`.
- The Cubism wrapper exposes parameter, part, drawable, canvas, offscreen, and
  diagnostic data without exposing raw sys pointers to the app layer.

### Rendering

- Metal rendering supports texture upload, drawable meshes, render order,
  normal/additive/multiplicative blend modes, per-drawable multiply/screen
  colors, double-sided culling flags, and optional atlas mipmaps/anisotropy.
- Clipping supports RGBA mask channel packing, 1/2/4/9 layout, explicit
  `matrix_for_mask` / `matrix_for_draw`, Cubism-style ppu precision branches,
  shared multi-texture masks, and optional high precision masks.
- Mask source generation uses source texture alpha without multiplying source
  drawable model opacity, preserving helper mask drawables such as Rice's eye
  masks.
- Cubism offscreen drawables are detected, logged, and routed through a
  first-pass Metal offscreen render/composite path.
- Nested offscreen flush order and extended blend snapshot timing are covered by
  unit tests.
- Extended Cubism v5 blend diagnostics decode color + alpha pairs, and the
  first-pass Metal extended blend shader samples render-target snapshots before
  compositing.
- The Metal layer frame, contents scale, drawable size, mask textures,
  offscreen textures, blend snapshots, and MSAA texture are synchronized from
  the current AppKit window bounds/backing scale before rendering.
- Layer edge antialiasing and 4x MSAA are enabled when supported.

### Motion, Expression, Physics, And Input

- `MotionController` owns per-frame parameter updates in this order: idle
  blink/breath, idle motion playback, optional expression, mouse/microphone
  input, physics output, TOML overrides, then `csmUpdateModel`.
- Motion first pass supports model3 motion references, idle motion selection,
  looping, and linear/bezier/stepped/inverse-stepped segments.
- Expression first pass supports `Add`, `Multiply`, and `Overwrite` parameter
  blends selected by local TOML config.
- Physics first pass supports input normalization, particles, gravity, wind,
  stabilization, fixed-step evaluation, output interpolation, and diagnostics.
- Input first pass supports mouse-driven head/eye parameters and microphone
  RMS-driven `ParamMouthOpenY`.

### macOS Windowing And Reliability

- The app opens a native transparent borderless AppKit window that can join all
  Spaces.
- Done: the root content layer is transparent by default, with no rounded dark
  panel background, so normal runs show only the Live2D model unless diagnostics
  are explicitly enabled.
- Done: startup window size is configurable through `[app].window_width` and
  `[app].window_height`; local dev/build configs use `540x720`, 1.5x the
  original prototype size.
- The render loop targets roughly 60 FPS and reports frame timing diagnostics.
- App startup uses a local PID guard under `target/vtube-studio-rs.pid` to avoid
  duplicate development windows. `VTUBE_RS_ALLOW_DUPLICATE_INSTANCE=1` is only
  for deliberate debugging.
- Renderer lifecycle events use `renderer_event=...` records for startup,
  drawable size changes, mask/offscreen/MSAA texture changes, drawable
  availability, AppKit active/visible/occlusion state changes, long frame gaps,
  and inferred display wake events.
- The first Space reliability baseline passed on 2026-05-20 using
  `target/space-test/space-test-20260520-191559.md`: startup guards, drawable
  recovery, and display wake checks passed; long frame gaps were transition
  signals.

### Local Tooling

- `cargo xtask run-metal` provides a one-command local Metal run path with SDK
  auto-detection for `public/CubismSdkForNative`; it compiles
  `metal-renderer camera-tracking` by default, launches through the local
  signed `.app` wrapper for stable camera permissions, and the active TOML
  controls whether camera capture starts. `cargo xtask run-metal --release`
  runs the optimized build profile against `vtube-studio-rs.build.toml`, where
  local camera input is enabled by default.
- `cargo xtask capture-full-matrix` runs the standard visual regression matrix
  and writes one final Markdown report.
- `cargo xtask run-space-test` writes Space/display reliability logs and a
  Markdown checklist report under `target/space-test/`.
- `cargo xtask clean --generated` removes generated local artifacts; `--all`
  also removes Cargo build output.
- Screenshot capture, window lookup, and stale process cleanup are implemented
  in Rust/macOS APIs instead of shelling out to `screencapture`, Swift, `tail`,
  or `pkill`.

## Validation And Regression Tooling

- `--probe-models` scans local `.model3.json` files without opening a window and
  reports parameter, part, drawable, masked drawable, maximum mask, blend-mode,
  inverted-mask, extended blend, and offscreen counts.
- `cargo xtask probe-risk-models` writes `target/render-regression/probe.txt`;
  the render regression report embeds it for each screenshot sweep.
- `cargo xtask sample-compatibility-sweep` scans official SDK sample resources
  and writes `target/render-regression/compatibility-sweep.md`.
- Current SDK sample probe loads 9 models successfully. `Mao` stresses dense
  clipping and multi shared mask textures. `Ren` stresses offscreen and
  extended blend rendering. `Rice` is optional stress coverage for additive,
  inverted-mask, translucent-drawable, and eye-mask behavior.
- `cargo xtask capture-mask-matrix` and `mao-mask-audit` cover Mao clipping
  parity.
- `cargo xtask capture-offscreen-matrix`, `ren-offscreen-audit`, and
  `ren-visual-diff` cover Ren offscreen, extended blend, and focused pixel diff
  review.
- `cargo xtask capture-rice-stress` and `rice-stress-audit` cover optional Rice
  stress behavior when that SDK sample is present.
- `cargo xtask capture-quality-matrix` and `quality-visual-diff` compare
  mipmaps off/on and anisotropy 1/8 for the default model, Mao, and Ren.
- `target/render-regression/report.md` is the main visual review index. It
  includes latest screenshots, contact sheets, manual review records, model risk
  probe output, fallback events, MSAA/edge quality summaries, Retina/resize
  stability summaries, focused audit summaries, and recent renderer events.

## Known Limitations

- This is not a full VTube Studio replacement yet.
- Camera tracking is first-pass rather than fully calibrated. The selected v1
  path uses macOS AVFoundation capture plus Vision face landmarks, and the
  local pipeline now starts/stops capture, extracts landmarks, maps samples into
  motion parameters, and exposes TOML/session calibration controls. Broader
  real-camera tuning is still needed. ARKit/iPhone and plugin bridges are
  deferred.
- There is no GUI settings panel yet; runtime controls are read from
  profile-specific TOML files before startup, with a first-pass macOS status
  bar menu now exposing current model/expression/renderer state and session
  toggles for diagnostics, mouse tracking, microphone input, camera tracking,
  and expression selection, plus runtime Soft/Normal/Expressive presets for
  mouse, mouth, and camera calibration.
- Motion, expression, physics, mouse, and microphone support are first-pass
  implementations; mouse/mouth drivers now expose model-specific calibration
  knobs, but still need real-device tuning.
- Offscreen, clipping, and extended blend parity are first-pass implementations
  and still need broader official sample validation.
- High precision masks currently fall back to shared masks when Cubism offscreen
  drawables are present; diagnostics show this as `mask shared(offscreen)`.
- Microphone startup failures now print actionable terminal guidance with the
  macOS permission path and active profile config names; native in-window
  prompting is still open.
- Packaging, app signing, auto-start, and menu bar controls are not implemented.
- Long-running Space/display sleep-wake reliability still needs more real-world
  reports beyond the first baseline.

## Next Product Phase

The next product phase should shift from renderer/tooling groundwork toward a
usable avatar host experience.

Priority 1: Model And Runtime Usability

- Add model selection and model switching instead of hard-coding a single local
  model path in daily use.
- Keep `cargo xtask list-models` available as the developer-facing model
  discovery path until an in-app picker exists.
- Keep `cargo xtask select-model [--dev|--build] MODEL_PATH` available as the
  developer-facing model selection path until an in-app picker exists.
- Keep `cargo xtask doctor` available to check dev/build config files, selected
  model manifests, window size settings, output mode, renderer, motion, mouse,
  microphone, and camera input settings, and Cubism Core SDK paths before
  launching.
- Validate the selected `.model3.json` before opening the avatar window and
  print the active config path plus repair commands when it is missing.
- Continue growing the first-pass status bar settings menu into a small
  settings UI. Local model selection, window size presets, renderer quality
  presets, diagnostics, expression selection, and input toggles are already
  available as menu controls.
- Keep improving missing SDK/model/microphone/camera permission messages so
  failures are visible to non-developer users beyond the terminal. The first
  pass now includes `VT` menu shortcuts to macOS Camera and Microphone privacy
  settings.

Priority 2: Tracking And Input

- Calibrate mouse tracking ranges per model using the local
  `coordinate_space`/`eye_*_range`/`angle_*_degrees`/dead-zone controls and the
  runtime Soft/Normal/Expressive preset menu. Default mouse tracking is
  screen-relative; `coordinate_space = "window"` remains available when window
  relative tracking is needed. Keep `cargo xtask tune-input` available to write
  persistent mouse/mouth/camera starter calibration profiles into the active
  dev/build TOML after trying runtime presets.
- Calibrate microphone gain/noise gate/response-curve/attack/release defaults
  on more machines using the runtime mouth preset menu as a first pass.
- Implement the selected camera-tracking v1 path: built-in/default webcam
  capture through AVFoundation plus Vision face landmarks.
- Add permission and setup messaging for camera/microphone paths.

Priority 3: Rendering Parity And Quality

- Continue validating dense clipping, inverted masks, offscreens, extended
  blends, mipmaps, anisotropy, MSAA, and Retina resize behavior against official
  samples and real user models.
- Use `compatibility-sweep.md` to decide when a new model should join the visual
  regression matrix.
- Refine nested offscreen, offscreen mask, and extended blend parity where
  official sample comparison reveals differences.

Priority 4: macOS Productization

- Continue Space/display sleep-wake reliability runs on real desktops.
- Add packaging, signing, menu bar controls, and optional launch-at-login.
- Keep duplicate-window prevention and generated-artifact cleanup reliable for
  day-to-day development.

## Non-Goals For The Next Phase

- Do not attempt full VTube Studio feature parity yet.
- Do not commit official Live2D SDK files or model assets to GitHub.
- Do not build a plugin marketplace or scene editor before rendering and motion
  are stable.
- Do not replace Cubism Core with a pure Rust `.moc3` runtime in this phase.

## Requirements

### 1. Motion, Expression, And Physics

Status: first pass done; calibration and compatibility ongoing.

Required outcomes:

- Parse motion file references from `model3.json` when present.
- Parse expression file references from `model3.json` when present.
- Parse `physics3.json` and create a runtime representation of inputs, outputs,
  particles, gravity, wind, weights, scales, and reflection flags.
- Keep idle breathing and blinking enabled by default.
- Keep local config overrides such as `overrides.mouth_open` useful for
  debugging.

Remaining work:

- Add broader compatibility tests against more official SDK sample models.
- Keep the official SDK sample compatibility sweep available as the first pass
  before adding new visual regression fixtures.
- Add better diagnostics for unsupported motion segment data.
- Add expression fade-in/fade-out if needed by later UI controls.

### 2. Parameter Drivers And Input

Status: first pass done; calibration ongoing.

Required outcomes:

- Mouse position drives `ParamEyeBallX`, `ParamEyeBallY`, `ParamAngleX`,
  `ParamAngleY`, and `ParamAngleZ`.
- Microphone level drives `ParamMouthOpenY`.
- Camera tracking v1 drives `ParamAngleX`, `ParamAngleY`, `ParamAngleZ`,
  `ParamEyeBallX`, `ParamEyeBallY`, and optionally `ParamMouthOpenY` from a
  local webcam face/landmark stream.
- Automatic blink and breathing remain enabled by default.
- Input drivers can be toggled for debugging.

Remaining work:

- Calibrate mouse tracking ranges per model.
- Calibrate microphone gain/noise gate/response-curve/attack/release defaults
  on more machines.
- Consider native macOS permission messaging if microphone startup fails; the
  terminal diagnostic path is now covered.
- Implement camera permission messaging and a no-camera fallback before enabling
  camera tracking by default.

### 2a. Camera Tracking V1 Requirements

Decision: implement camera tracking v1 as a macOS-native webcam tracker using
AVFoundation for camera capture and Vision for face rectangle/landmark
extraction. Do not use ARKit/iPhone capture or a MediaPipe/plugin bridge in v1.
Do not continue expanding hand-written Objective-C FFI for the camera backend;
move the native camera implementation to the `objc2` ecosystem, specifically
`objc2-av-foundation`, `objc2-vision`, and `block2`, while keeping a small safe
Rust wrapper between Apple framework objects and the rest of the app.

Goals:

- Keep the full camera path local to the machine. Do not store frames, write
  camera images to disk, or include image data in logs.
- Use the default camera by default, with an optional device name/id override
  later.
- Target 30 FPS camera sampling without blocking the 60 FPS render loop.
- If no camera is available, permission is denied, or Vision cannot find a
  face, the avatar should continue rendering with idle blink/breath and any
  enabled mouse/microphone inputs.
- Expose camera state in the `VT` status bar menu and diagnostics overlay:
  disabled, waiting for permission, running, no face, no camera, or failed.

Parameter mapping:

- `VNDetectFaceRectanglesRequest` revision 3 yaw/pitch maps to `ParamAngleX`
  and `ParamAngleY`; if native pose is missing, estimate yaw from nose/eye
  landmarks, then fall back to face center offset. Normalize roughly 30 degrees
  of real yaw/pitch to the full configured Live2D head range so native camera
  tracking has visible movement without requiring exaggerated head turns.
- `VNDetectFaceRectanglesRequest` revision 3 roll maps to `ParamAngleZ`, with
  roughly 45 degrees of real roll mapping to the full configured tilt range.
- Camera `invert_x` defaults to `true` so the native camera path behaves like a
  mirrored avatar preview; users can set it to `false` for already mirrored
  capture pipelines.
- Face/landmark gaze approximation maps to `ParamEyeBallX` and
  `ParamEyeBallY`; if landmarks are missing, fall back to face center offset.
- Mouth landmark opening maps to `ParamMouthOpenY` when camera mouth tracking
  is enabled. If microphone mouth input is also enabled, use a configurable
  combine mode, defaulting to `max(camera, microphone)`.
- Mouse and camera both drive head/eye pose parameters, so they must not write
  those parameters in the same frame without an explicit policy. Add
  `[input.camera].pose_mode` with `camera_when_available`, `camera`, and
  `mouse`; default to `camera_when_available` so camera owns head/eye pose only
  when a face sample exists, with mouse as fallback.
- Eye landmark openness may drive blink/eye-open parameters when
  `blink_from_camera` is enabled; close/open thresholds add hysteresis and
  automatic blink remains the fallback when camera blink is disabled. When
  camera blink is enabled, neutralize eye-open parameters while lightweight
  physics samples inputs, then restore the real blink value before Cubism
  update; this prevents models that wire eye-open into physics from producing a
  blink-triggered secondary body/part pulse.

Configuration:

```toml
[input.camera]
enabled = false
device = ""
target_fps = 30
pose_mode = "camera_when_available"
smoothing = 12.0
dead_zone = 0.03
invert_x = true
invert_y = false
face_x_offset = 0.0
face_y_offset = 0.0
gaze_x_offset = 0.0
gaze_y_offset = 0.0
roll_offset = 0.0
angle_x_degrees = 30.0
angle_y_degrees = 22.0
angle_z_degrees = 12.0
eye_x_range = 1.0
eye_y_range = 1.0
mouth_enabled = true
mouth_gain = 1.4
mouth_open_offset = 0.0
mouth_min_open = 0.0
mouth_max_open = 1.0
mouth_combine = "max"
blink_from_camera = false
blink_close_threshold = 0.20
blink_open_threshold = 0.38
```

Implementation plan:

- Done: add `CameraConfig` under `[input.camera]` with defaults and README
  examples.
- Done: add a safe Rust-facing camera input scaffold plus `VT` menu and
  diagnostics status lines for the native backend.
- Done: add the optional `camera-tracking` feature and a macOS AVFoundation
  permission/device probe that reports disabled, waiting for permission,
  permission denied, no camera, backend pending, or failed without starting
  frame capture.
- Done: add a safe camera motion sample contract and merge path into
  `MotionController` for head angle, eye-ball, optional camera blink, and
  camera/microphone mouth combining.
- Done: replace the current hand-written camera Objective-C probe with
  `objc2-av-foundation`, `objc2-vision`, and `block2` dependencies under the
  optional `camera-tracking` feature. The probe now uses typed
  `objc2-av-foundation` calls for authorization and device lookup, with Vision
  landmark extraction contained in the native camera module.
- Done: add a non-running AVFoundation capture session build check under
  `camera-tracking`: create `AVCaptureDeviceInput`, create
  `AVCaptureVideoDataOutput`, add both to an `AVCaptureSession`, and commit the
  configuration without starting capture or retaining sample buffers.
- Done: wire an `AVCaptureVideoDataOutputSampleBufferDelegate` through
  `objc2-av-foundation`, `objc2-core-media`, and `dispatch2`. The delegate is
  held by the native capture pipeline, runs throttled Vision landmark requests,
  and does not retain, write, or log frame image data.
- Done: make `CameraInput` hold the native capture runtime when
  `camera-tracking` is enabled and permission/device setup succeeds. The
  runtime starts/stops `AVCaptureSession`, reports `running` / `no face` /
  `failed`, and clears the sample buffer delegate on drop.
- Done: request macOS camera access when AVFoundation reports
  `NotDetermined`, log `renderer_event=camera_permission_response`, and keep
  rendering without camera tracking until the user grants permission and
  restarts.
- Done: make `cargo xtask run-metal` the single local launch path for dev and
  release: it builds `metal-renderer camera-tracking`, installs the binary into
  `target/dev-app/vtube-studio-rs Dev.app`, launches through LaunchServices with
  `--config`, and uses the stable `rs.vtube-studio.dev` identity for camera
  permissions.
- Done: keep the `.app` path stable and code sign it after each build using
  `VTUBE_RS_CODESIGN_IDENTITY` or a detected local development identity when
  available, with ad-hoc signing as a fallback.
- Done: add the first Vision request call site inside the sample buffer
  delegate. Frames are throttled by `[input.camera].target_fps`, processed with
  `VNDetectFaceLandmarksRequest`, and summarized as face/no-face/failure
  events without storing or logging image data.
- Done: map first-pass Vision output into `CameraMotionSample`: bounding box
  center drives face offset, roll drives face roll, pupil-vs-eye geometry
  drives gaze, eye vertical ratio drives optional eye openness, and lip
  vertical ratio drives mouth openness.
- Done: surface camera calibration data in diagnostics and the `VT` menu:
  status, face offset, roll, gaze, mouth openness, and eye openness update while
  the app is running so real-camera tuning has visible feedback.
- Done: add `VT` menu Soft/Normal/Expressive camera calibration presets that
  scale camera head/eye ranges and mouth gain for the current session.
- Done: add `cargo xtask tune-input [--dev|--build] <mouse|mouth|camera>
  <soft|normal|expressive>` so session calibration choices can be persisted to
  profile TOML without hand-editing every range/gain field.
- Done: add a `VT` menu Camera Tracking toggle that can start/stop the native
  camera capture lifecycle during the current session and keeps the motion
  layer's camera driver in sync with the selected camera calibration preset.
- Done: surface actionable camera status details in diagnostics and the `VT`
  menu for permission denied, no camera, no face, backend missing, and failed
  setup states.
- Done: add normalized camera calibration offsets for face center, gaze, roll,
  and mouth-open zero point so real webcams and models can be tuned without
  changing the Vision landmark parser.
- Done: prefer Vision yaw/pitch for camera head rotation and add camera blink
  threshold hysteresis so eye-open landmarks can drive blinks without
  half-open flutter.
- Continue calibrating the Vision landmark-to-parameter mapping on real cameras
  and model assets.

Non-goals for v1:

- No ARKit blendshape stream.
- No iPhone/Continuity Camera-specific protocol.
- No identity recognition, recording, frame export, or background upload.
- No full facial expression classifier beyond the landmark-derived parameters
  listed above.

Acceptance criteria:

- With `[input.camera].enabled = false`, builds and runtime behavior are
  unchanged.
- With camera tracking enabled and permission granted, head angle and eye
  parameters respond smoothly to face movement.
- With no face detected, parameters decay smoothly back toward neutral instead
  of freezing at the last extreme.
- Denied camera permission produces actionable setup text and leaves the avatar
  running.
- Space switching, display sleep/wake, and duplicate-window prevention continue
  to work while the camera session is active.

### 3. Engineering Experience

Status: first pass done; maintenance ongoing.

Required outcomes:

- Document exact `public/` sample directory layout.
- Document that `public/` is intentionally ignored and not uploaded to GitHub.
- Auto-detect `public/CubismSdkForNative` SDK paths when explicit env vars are
  absent.
- Keep one-command local run and regression paths under `cargo xtask`.
- Keep diagnostics overlay visibility and runtime debug controls in local TOML
  config rather than process environment variables.
- Split local runtime config into development and build files:
  `vtube-studio-rs.dev.toml` and `vtube-studio-rs.build.toml`.
- Keep `app.runtime_profile = "development" | "release"` available so release
  style runs can default renderer event logs and MSAA off without removing
  Cubism, clipping, or offscreen correctness paths.
- Keep README and PRD aligned as capabilities change.

Acceptance criteria:

- A developer with local model files and SDK can run the Metal renderer from
  README instructions.
- Missing SDK/model files produce actionable messages.
- Debug overlay can be hidden/shown without editing code.

### 3a. macOS Native API Binding Strategy

Status: next engineering phase. Camera capture has already moved onto the
`objc2` ecosystem; the remaining work is to migrate the AppKit/Foundation
surface in controlled slices without changing product behavior.

Decision: use the `objc2` ecosystem as the preferred Rust bridge for macOS
native APIs going forward. The project should stop growing broad hand-written
Objective-C `objc_msgSend` bindings except for small, isolated compatibility
shims.

Migration scope:

- Done: migrate `src/macos_camera.rs` to `objc2-av-foundation`,
  `objc2-vision`, `objc2-foundation`, and `block2` for authorization, device
  lookup, capture session setup, sample buffer delegates, and Vision landmark
  extraction.
- Next: migrate AppKit/Foundation usage in `src/macos_app.rs` to
  `objc2-app-kit`, `objc2-foundation`, `objc2-quartz-core`, and
  `objc2-core-graphics` where practical. This includes `NSApplication`,
  `NSPanel`/`NSWindow`, status/menu items, `CATextLayer`, `CALayer`,
  `CAMetalLayer` attachment, CoreGraphics window levels, and window/Space
  behavior.
- Keep Cubism Core on `live2d-cubism-core-sys`; `objc2` does not replace the
  Cubism C API.
- Keep Metal rendering on the existing `metal` crate for now. Revisit only if
  renderer maintenance requires a broader Apple framework migration.
- Keep microphone input on `cpal` unless the project later needs macOS-specific
  audio device or permission control.
- Keep current `objc_msgSend` shims only as temporary compatibility wrappers
  while a specific subsystem is migrated; do not add new broad untyped shims for
  new feature work.

Migration order:

1. Done: replace the hand-written camera Objective-C probe with `objc2` crates.
2. Done: implement AVFoundation capture and Vision landmark extraction behind
   safe Rust-facing camera structs.
3. Done: add a small internal Apple platform helper layer for shared
   conversions such as Foundation strings, `NSError` descriptions, and
   local-only diagnostic messages.
4. Done: migrate status bar/menu/settings UI pieces from hand-written AppKit
   FFI to `objc2-app-kit`: typed `NSMenu` and `NSMenuItem` creation, `addItem`,
   separator items, enabled state, submenu assignment, title/state/tag updates,
   target assignment, `NSStatusBar` / `NSStatusItem` installation with typed
   status button title and tooltip, and settings action target class
   registration via `objc2::runtime::ClassBuilder`. The remaining raw
   `objc_msgSend` usage in `src/macos_app.rs` now belongs to later window,
   layer, event-pump, and compatibility slices.
5. Done: migrate layer creation and frame updates to typed
   `objc2-quartz-core` and `objc2-foundation` wrappers where practical:
   diagnostics `CATextLayer` creation, foreground color assignment, frame,
   font size, contents scale, z-position, wrapping, string updates, hidden
   state, diagnostic sublayer insertion, view-backed root `CALayer`
   configuration, software placeholder `CALayer` creation/style/position
   updates, software placeholder `NSImage` contents loading, `CATransaction`
   begin/commit with disabled implicit actions, root layer bounds reads, and
   Metal layer frame/contents-scale/attachment compatibility wrappers. The
   Metal layer object itself remains owned by the `metal` crate.
6. Done: migrate transparent `NSPanel` creation and CoreGraphics window level
   lookup to typed `objc2-app-kit` / `objc2-core-graphics` wrappers. The
   wrapper owns AppKit panel allocation, transparent background configuration,
   non-activating/floating panel flags, content view lookup, backing scale
   lookup, and Space policy reapplication while preserving the current
   `screen_saver`, `canJoinAllSpaces`, `canJoinAllApplications`, `stationary`,
   `ignoresCycle`, and `fullScreenAuxiliary` behavior.
7. Done: migrate the event pump and lifecycle polling to typed
   `objc2-app-kit` wrappers in small slices. The platform layer now owns
   `NSDate::distantPast`, nonblocking `nextEventMatchingMask`, `sendEvent`,
   `updateWindows`, `NSApplication::isActive`, `NSWindow::isVisible`,
   `NSWindow::occlusionState`, and `orderFrontRegardless`, preserving the
   existing Space reassertion behavior and reliability diagnostics.
8. Done: migrate Finder/System Settings launch helpers from `/usr/bin/open`
   process spawning to typed `NSWorkspace` wrappers. Privacy pane links,
   active-model reveal, and local models folder opening now live in the
   platform layer through `objc2-app-kit` plus `objc2-foundation` URL helpers.

Acceptance criteria:

- `cargo fmt --check`, `cargo test --features camera-tracking`, and
  `cargo test --features "metal-renderer camera-tracking"` pass after every
  slice.
- `cargo xtask run-metal --release` still launches through the stable local
  `.app` identity, preserves camera permission behavior, and shows only the
  transparent Live2D model by default.
- The `VT` menu still supports diagnostics, model selection, window size
  presets, expression selection, input toggles, and calibration presets.
- Space reliability behavior is not regressed: no duplicate windows, startup
  guard remains active, and `window_configured` logs the expected panel policy.
- Any remaining hand-written Objective-C shim has a narrow owner and a clear
  TODO pointing to the subsystem that will replace it.

### 4. CubismClippingManager And Offscreen Parity

Status: first pass done; parity refinement ongoing.

Required outcomes:

- Preserve RGBA channel packing and 1/2/4/9 layout behavior.
- Keep explicit `matrix_for_mask` and `matrix_for_draw` structures.
- Keep masked offscreen composites using inverse-fit model positions for mask
  matrix sampling.
- Keep Cubism Core offscreen drawable detection in runtime diagnostics.
- Continue validating Metal offscreen render/composite behavior against `Ren`
  and future offscreen-heavy sample models.
- Keep nested offscreen flush order and extended blend snapshot timing covered
  by tests.
- Refine nested offscreen, offscreen mask, and extended blend parity where
  official sample comparison reveals differences.

Acceptance criteria:

- Existing sample models render without atlas text artifacts or white-eye
  regressions.
- Masked eye and mouth drawables remain visually stable after window resizing.
- Renderer logs explain fallback causes when mask count or offscreen features
  exceed current support.

### 5. Rendering Quality

Status: active validation pass.

Required outcomes:

- Keep normal, additive, and multiplicative blending visually consistent with
  Cubism Framework behavior.
- Keep per-drawable multiply and screen colors enabled for masked and unmasked
  drawables.
- Keep culling, double-sided drawables, inverted masks, clipped drawables,
  optional mipmaps, anisotropy, MSAA, and Retina resize behavior stable across
  visual regression matrices.
- Keep renderer-owned texture memory diagnosable through `renderer_event=memory_budget`
  with atlas, mask, offscreen, MSAA, snapshot, and total estimates.
- Keep atlas mipmaps optional: disabled by default to avoid atlas island bleed,
  but available for well-padded model textures.
- Keep transparent window edges and avatar mesh edges smooth with layer edge
  antialiasing and MSAA where supported.

Acceptance criteria:

- Additive, multiplicative, normal, masked, inverted-mask, offscreen, and
  extended-blend drawables remain correct in standard sample matrices.
- Texture sampling remains crisp without obvious shimmer, blur, or atlas island
  bleed at common window sizes.
- Transparent window corners and avatar edges avoid obvious stair-step artifacts.
- Large RSS values can be compared against renderer-owned texture memory before
  treating them as leaks.

### 6. macOS Space Stability

Status: first reliability pass done; long-run validation ongoing.

Required outcomes:

- Verify Space switching with the avatar visible on all Spaces.
- Verify behavior beside full-screen apps.
- Confirm App Nap prevention is sufficient during active rendering.
- Done: refine overlay Space policy to a non-activating `NSPanel` at
  configurable CoreGraphics window level plus
  `NSWindowCollectionBehaviorCanJoinAllSpaces`,
  `NSWindowCollectionBehaviorCanJoinAllApplications`, `stationary`,
  `ignoresCycle`, and `fullScreenAuxiliary`, and reassert ordering when AppKit
  reports the avatar window hidden or not visibly occluded.
- Done: default the overlay to `app.window_level = "screen_saver"` following
  Apple DTS guidance for non-activating overlay panels above full-screen Spaces;
  `overlay` and `maximum` remain available for matrix testing.
- Rejected: `NSWindowCollectionBehaviorTransient` caused hidden/double-image
  frames during Space swipe videos, so the default policy avoids it.
- Rejected: private WindowServer/CGS sticky tags were tested and made the window
  appear only on one Desktop, so they are not part of the default runtime.
- Done: switch AppKit event polling to non-blocking default, event-tracking, and
  modal run loop modes so Space gestures are less likely to stall the render
  loop.
- Detect and recover from Metal layer/device issues after display sleep/wake.
- Add structured trace output for long frame gaps and Space/display transitions.
- Log AppKit active, window visibility, and window occlusion state changes while
  running reliability tests.

Acceptance criteria:

- Frame count continues increasing during repeated Space switches.
- FPS recovers after transitions.
- No duplicate app windows remain after reruns.
- Display sleep/wake does not permanently blank the avatar.

Baseline:

- `target/space-test/space-test-20260520-191559.md`
- Automatic assessment: startup guards PASS, drawable recovery PASS, display
  wake PASS, long frame gaps INFO.
- Event counts: `window_occlusion_changed=20`, `app_active_changed=11`,
  `long_frame_gap=2`, `next_drawable_unavailable=0`,
  `display_wake_inferred=0`.

## Milestone Plan

### Milestone A: Runtime Parameter Layer

Status: first pass done.

- `motion.rs` owns per-frame parameter updates.
- Parameter getters/setters by ID are available through the Cubism wrapper.
- Mouse-driven eye/head controls are implemented.
- Microphone mouth driver is implemented.

### Milestone B: Motion And Expression Files

Status: first pass done.

- `Live2dModel` parses motions and expressions from `model3.json`.
- Minimal `.motion3.json` curve evaluation is implemented.
- `.exp3.json` parameter add/multiply/overwrite support is implemented.
- Idle motion selection and looping are implemented.

### Milestone C: Physics

Status: first pass done.

- `physics3.json` is parsed.
- Input normalization, particle simulation, stabilization, fixed-step updates,
  and output interpolation are implemented.
- Diagnostics log physics setting/output counts.

### Milestone D: Engineering Experience

Status: first pass done; maintenance ongoing.

- README/PRD are kept updated.
- Local developer tooling runs through Cargo-managed `cargo xtask ...` commands
  instead of repository shell scripts.
- Text reports, visual diffs, capture matrix orchestration, window lookup,
  screenshot capture, log tailing, and stale process cleanup are implemented in
  Rust.
- Main commands remain `cargo xtask run-metal`, `cargo xtask run-metal
  --release`, `cargo xtask capture-full-matrix`, `cargo xtask run-space-test`,
  and `cargo xtask clean --generated`.

### Milestone E: Rendering And Clipping Quality

Status: active validation pass.

- Standard visual matrices cover baseline models, Mao clipping, Ren offscreen,
  optional Rice stress, texture sampling, anisotropy, MSAA edge quality, and
  Retina/window resize logs.
- Continue adding stress models only when compatibility sweep finds a new risk
  shape not already covered by Mao, Ren, or Rice.
- Continue refining clipping/offscreen/extended blend parity based on visual
  reports and official sample comparison.

### Milestone F: macOS Reliability Pass

Status: first pass done; repeat validation ongoing.

- Keep the repeatable Space-switch checklist and Markdown report workflow.
- Use `cargo xtask run-space-test` for future display sleep/wake recovery tests.
- Extend structured renderer lifecycle logging as new macOS failure cases are
  found.
- Continue reducing duplicate process/window issues during development.

### Milestone G: Product Usability

Status: next product milestone.

- Add model selection and switching.
- Keep startup model selection available through `[model].path` in
  `vtube-studio-rs.dev.toml` / `vtube-studio-rs.build.toml`; command-line model
  paths override TOML for one run.
- Done: when the active dev/build TOML is missing, startup creates it from the
  matching committed example config before loading. If the example is also
  missing, the app still falls back to built-in defaults.
- Reuse the local model discovery path currently exposed by
  `cargo xtask list-models`.
- Reuse the local model selection path currently exposed by
  `cargo xtask select-model [--dev|--build] MODEL_PATH`.
- Done: expose a first-pass `VT` menu local model list. It scans `public/`,
  writes the selected `.model3.json` to the active profile TOML, relaunches the
  local `.app` with that selected model, and avoids stale command-line model
  overrides; full in-process hot switching remains planned.
- Done: expose `VT` menu `Reveal Active Model...` so users can locate the
  loaded `.model3.json` in Finder and inspect local `public/` resources without
  reading terminal paths.
- Done: expose `VT` menu `Open Models Folder...`; it opens the local `public/`
  folder and creates it first when missing, so model resource setup does not
  require terminal directory work.
- Done: expose `VT` menu Window Size presets. Selecting 100%, 125%, 150%, or
  200% writes `[app].window_width` / `[app].window_height` to the active
  dev/build TOML and relaunches the local `.app`.
- Removed: Syphon is no longer a product path. The app now focuses on normal
  transparent macOS window output for OBS Window Capture and macOS Screen
  Capture. Future high-performance frame sharing should evaluate
  ScreenCaptureKit, Metal, or IOSurface instead of restoring Syphon.
- Done: expose first-pass `VT` menu Renderer Quality presets. Selecting
  Performance, Balanced, or High Quality writes `[renderer]` quality fields to
  the active dev/build TOML and relaunches the local `.app`; full in-process
  renderer reconfiguration remains planned.
- Continue expanding the status bar controls into a broader settings surface.
  The first-pass `VT` menu already shows renderer/model state and controls
  diagnostics, expression selection, mouse tracking, microphone input, camera
  tracking, input calibration presets, local model selection, window size, and
  renderer quality.
- Done: add first-pass `VT` menu shortcuts for Camera and Microphone privacy
  settings so users can repair macOS permission issues without searching
  terminal logs.
- Continue improving user-facing permission and missing-file messages beyond
  the current startup terminal diagnostics.
- Prototype the selected AVFoundation + Vision camera-tracking path and expose
  it through `[input.camera]` plus the `VT` status bar menu.
- Done: add `cargo xtask build-app [--release]` as the first packaging/signing
  entry point. It builds the Metal + camera app wrapper, signs it with the same
  stable local bundle identity as `run-metal`, prints the active profile config,
  and does not launch the app.
- Done: extend `cargo xtask doctor` to validate `[app].window_width` and
  `[app].window_height` so profile config mistakes are caught before launching
  the avatar window.
- Done: extend `cargo xtask doctor` to validate common `[input.mouse]`,
  `[input.microphone]`, and `[input.camera]` mode/range mistakes, including
  `coordinate_space`, `pose_mode`, `mouth_combine`, camera FPS, mouth/blink
  thresholds, smoothing, dead zones, ranges, and angle limits.
- Done: extend `cargo xtask doctor` to validate common `[renderer]` and
  `[motion]` mistakes, including `debug_texture_mode`, atlas anisotropy, blink
  interval/duration, and empty expression overrides.
- Done: keep `cargo xtask doctor` focused on active window, model, renderer,
  motion, input, and Cubism Core SDK checks. Legacy `[output]` settings are no
  longer validated because Syphon output has been removed.
- Continue packaging/signing/launch-at-login decisions; launch-at-login remains
  intentionally unimplemented until the app bundle/install location is settled.
- Start the objc2 AppKit/Foundation migration phase from section 3a before
  adding more large macOS UI features, so future menu/settings/window work is
  built on typed framework bindings instead of expanding hand-written
  `objc_msgSend` wrappers.

## Debug Controls

Runtime debug controls are read once at startup from local profile configs.
Development runs use `vtube-studio-rs.dev.toml`; release builds use
`vtube-studio-rs.build.toml`. Both files are ignored by Git; committed defaults
live in `vtube-studio-rs.dev.example.toml` and
`vtube-studio-rs.build.example.toml`.

```toml
[app]
runtime_profile = "development"
window_level = "screen_saver"
window_width = 540.0
window_height = 720.0

[model]
path = "public/model/0.model3.json"

[diagnostics]
show = false

[renderer]
disable_masks = false
high_precision_masks = false
# Defaults depend on [app].runtime_profile:
# development => enable_msaa/log_events true, release => false.
# enable_msaa = true
# log_events = true
atlas_mipmaps = false
atlas_anisotropy = 1
debug_texture_mode = "none"
hidden_drawables = []
hidden_parts = []
only_drawables = []
only_parts = []
highlight_drawables = []
highlight_parts = []

[motion]
# expression = "smile"
blink_interval = 3.8
blink_duration = 0.18

[input.mouse]
enabled = false
coordinate_space = "screen"
smoothing = 10.0
dead_zone = 0.02
invert_x = false
invert_y = false
eye_x_range = 1.0
eye_y_range = 1.0
angle_x_degrees = 30.0
angle_y_degrees = 22.0
angle_z_degrees = -12.0

[input.microphone]
enabled = false
parameter = "ParamMouthOpenY"
gain = 10.0
noise_gate = 0.008
response_curve = 0.6
smoothing = 18.0
attack = 32.0
release = 10.0
min_open = 0.0
max_open = 1.0

[input.camera]
enabled = false
device = ""
target_fps = 30
smoothing = 12.0
dead_zone = 0.03
invert_x = true
invert_y = false
face_x_offset = 0.0
face_y_offset = 0.0
gaze_x_offset = 0.0
gaze_y_offset = 0.0
roll_offset = 0.0
angle_x_degrees = 30.0
angle_y_degrees = 22.0
angle_z_degrees = 12.0
eye_x_range = 1.0
eye_y_range = 1.0
mouth_enabled = true
mouth_gain = 1.4
mouth_open_offset = 0.0
mouth_min_open = 0.0
mouth_max_open = 1.0
mouth_combine = "max"
blink_from_camera = false
blink_close_threshold = 0.20
blink_open_threshold = 0.38

[overrides]
# mouth_open = 1.0
# mouth_form = 0.0
```

## Open Questions

- Which motion should be treated as the default idle motion when multiple
  `Idle` motions exist?
- Should microphone permission failures get an in-window prompt now that
  terminal diagnostics are in place?
- After AVFoundation + Vision camera tracking v1 lands, should ARKit/iPhone or
  plugin bridges become the next tracking input?
- How far should the project port official Cubism Framework physics beyond the
  current lightweight implementation?
- Should the renderer support software fallback long term, or keep it as a
  diagnostic path only?
