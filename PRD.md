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
  auto-detection for `public/CubismSdkForNative`.
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
- Webcam or ARKit face tracking is not implemented.
- There is no GUI settings panel yet; runtime controls are read from
  profile-specific TOML files before startup.
- Motion, expression, physics, mouse, and microphone support are first-pass
  implementations and need more model/device calibration.
- Offscreen, clipping, and extended blend parity are first-pass implementations
  and still need broader official sample validation.
- High precision masks currently fall back to shared masks when Cubism offscreen
  drawables are present; diagnostics show this as `mask shared(offscreen)`.
- Microphone permission failures currently need better user-facing messaging.
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
- Add a small settings UI or menu surface for diagnostics, renderer quality,
  input toggles, selected expression, and local model selection.
- Improve missing SDK/model/microphone permission messages so failures are
  visible to non-developer users.

Priority 2: Tracking And Input

- Calibrate mouse tracking ranges per model.
- Calibrate microphone gain/noise gate defaults on more machines.
- Decide whether face tracking should use built-in webcam, ARKit, or a plugin
  bridge first.
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
- Automatic blink and breathing remain enabled by default.
- Input drivers can be toggled for debugging.

Remaining work:

- Calibrate mouse tracking ranges per model.
- Calibrate microphone gain/noise gate defaults on more machines.
- Consider native macOS permission messaging if microphone startup fails.

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
- Keep or refine `NSWindowCollectionBehaviorCanJoinAllSpaces`, `stationary`,
  and `fullScreenAuxiliary`.
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
- Main commands remain `cargo xtask run-metal`, `cargo xtask capture-full-matrix`,
  `cargo xtask run-space-test`, and `cargo xtask clean --generated`.

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
- Reuse the local model discovery path currently exposed by
  `cargo xtask list-models`.
- Reuse the local model selection path currently exposed by
  `cargo xtask select-model [--dev|--build] MODEL_PATH`.
- Add settings UI or menu bar controls for diagnostics, renderer quality, input
  toggles, expression selection, and selected model.
- Improve user-facing permission and missing-file messages.
- Decide and prototype the face-tracking path.
- Prepare packaging/signing/launch-at-login decisions.

## Debug Controls

Runtime debug controls are read once at startup from local profile configs.
Development runs use `vtube-studio-rs.dev.toml`; release builds use
`vtube-studio-rs.build.toml`. Both files are ignored by Git; committed defaults
live in `vtube-studio-rs.dev.example.toml` and
`vtube-studio-rs.build.example.toml`.

```toml
[app]
runtime_profile = "development"

[model]
path = "public/model/0.model3.json"

[diagnostics]
show = true

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
smoothing = 10.0

[input.microphone]
enabled = false
gain = 7.0
noise_gate = 0.025
smoothing = 18.0

[overrides]
# mouth_open = 1.0
# mouth_form = 0.0
```

## Open Questions

- Which motion should be treated as the default idle motion when multiple
  `Idle` motions exist?
- Should microphone permission failures get an in-window prompt or only terminal
  diagnostics for now?
- Should webcam/face tracking be built in, bridged from ARKit, or exposed
  through a plugin API first?
- How far should the project port official Cubism Framework physics beyond the
  current lightweight implementation?
- Should the renderer support software fallback long term, or keep it as a
  diagnostic path only?
